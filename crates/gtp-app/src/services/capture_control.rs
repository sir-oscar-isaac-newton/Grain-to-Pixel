//! The capture commands (D-04): light and camera control, the test shot, the automatic level
//! calibration (H-17) and the capture job's start, on top of the shared light (`shared_light`), the busy flag and the camera slot.
//!
//! Two flags keep the hardware safe (docs/specification.md section 6.1):
//!
//! - **Busy**: held for the whole of a capture job, a test shot, a `light_connect` and a
//!   `light_set`. While it is held the others are refused with "A capture is in progress". The
//!   guard releases the flag on every way out, a panic included. `light_off` and the watchdog
//!   ignore it.
//! - **Tripped** (in `SharedLight`): the temperature watchdog switched the light off.
//!
//! The light is locked per call, never for a job, so the watchdog gets through while a capture
//! waits for the camera. A trip is noticed by the running job at its next colour (`set_levels` is
//! refused) or, after the last colour, when its picture arrives (`StepCamera`): the job then fails
//! and stores nothing.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::Duration;

use gtp_capture::calibration::{calibrate, percentile, PERCENTILE};
use gtp_capture::settings::clean_value;
use gtp_capture::{
    CameraFinder, CameraInfo, CameraPreview, CameraSetting, CameraSettingKind, CameraSettings,
    CameraSource, CaptureProfile, CaptureSequence, CapturedFile, ChannelMeter, LightGuard,
    LightInfo, LightSource, LightStatus, Pause, PortInfo, Region, ThreadPause, ZoomStep,
    CAPTURE_TIMEOUT,
};
use gtp_core::ImageF32;
use gtp_core::{
    AppError, ErrorCode, ExposureInfo, FrameId, JobId, JobResult, RollId, TripletChannel,
};
use gtp_ingest::{read_exposure_info, CLIP_LEVEL};
use gtp_io::{demosaic_channel, Limits, RawDecoder};
use serde::Serialize;

use crate::context::AppContext;
use crate::hardware::Hardware;
use crate::jobs::{JobCtx, JobKind, JobManager, LibraryEvents};
use crate::services::capture::{capture_triplet, safe_name};
use crate::services::capture_levels::{
    exposure_from_exif, levels_for_roll, levels_from_wide, LevelsUsed,
};
use crate::services::roll_frame;
use crate::shared_light::{tripped_error, LightReading, SharedLight};

/// How long a test shot waits for the light's first temperature reading.
const READING_WAIT: Duration = Duration::from_secs(1);
/// A wait for a reading is made of pauses of this length.
const READING_SLICE: Duration = Duration::from_millis(50);
/// The longest port name accepted by `light_connect`, in characters.
const MAX_PORT_CHARS: usize = 256;

/// Where a captured picture is in its step, for the `capture://step` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum StepState {
    /// The colour is on, settled, and the camera is about to be armed.
    Lit,
    /// The camera was told to take the picture.
    Fired,
    /// The picture arrived.
    Received,
}

/// Receives what the capture hardware reports. Called from the job thread (steps) and from the
/// watchdog thread (status), so it must be `Send + Sync`.
pub trait CaptureEvents: Send + Sync {
    /// `capture://step`: one channel moved to `state`.
    fn step(&self, channel: TripletChannel, state: StepState);
    /// `light://status`: what the light reported on a watchdog tick.
    fn light_status(&self, reading: &LightReading);
}

/// What a test shot measured on the channel that was lit.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TestShot {
    /// 256 bins of the lit channel's linear value (0 to 1 over the sensor's range), so levels can
    /// be balanced by hand: a peak in the last bins means the exposure is clipped.
    pub histogram: Vec<u32>,
    /// The share of photosites at 99 percent of full scale or more, in percent.
    pub clipped_pct: f32,
    /// The shutter time, ISO and aperture of the test picture, as its EXIF says; saved with the
    /// levels so that a capture with other settings can warn.
    pub exposure: ExposureInfo,
}

/// The flag that says the light and the camera are in use.
#[derive(Clone, Default)]
pub struct BusyFlag(Arc<AtomicBool>);

/// Holds the [`BusyFlag`]; releases it when dropped, whichever way the holder ends.
pub struct BusyGuard(Arc<AtomicBool>);

impl BusyFlag {
    /// Takes the flag, or says in words that a capture is in progress.
    pub fn acquire(&self) -> Result<BusyGuard, AppError> {
        match self
            .0
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => Ok(BusyGuard(Arc::clone(&self.0))),
            Err(_) => Err(busy()),
        }
    }

    /// True while a guard exists.
    pub fn is_busy(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn busy() -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "A capture is in progress. Wait for it to finish, then try again.",
    )
}

/// How a slot knows which camera there is.
enum Known {
    /// A camera that is always there (the fake one): listed as given.
    Fixed(CameraInfo),
    /// A camera found when it is needed (libgphoto2): the finder asks the backend, and the last
    /// list it gave is kept for the moments the backend cannot be asked (a picture is being taken).
    Found {
        finder: Box<dyn CameraFinder>,
        last: Mutex<Vec<CameraInfo>>,
    },
}

/// A camera the app can use, and the lock that gives one job the camera at a time.
pub struct CameraSlot {
    known: Known,
    /// Reads and changes the exposure settings; `None` for a camera that has no such access.
    settings: Option<Box<dyn CameraSettings>>,
    /// Reads live-view frames; `None` for a camera that has no live view.
    preview: Option<Box<dyn CameraPreview>>,
    camera: Mutex<Box<dyn CameraSource + Send>>,
}

impl CameraSlot {
    /// A slot holding `camera`, listed as `info`.
    pub fn new(info: CameraInfo, camera: Box<dyn CameraSource + Send>) -> Self {
        Self {
            known: Known::Fixed(info),
            settings: None,
            preview: None,
            camera: Mutex::new(camera),
        }
    }

    /// A slot whose camera may be plugged in after the app started: `finder` lists it and opens it
    /// when a capture or the list needs it.
    pub fn with_finder(
        camera: Box<dyn CameraSource + Send>,
        finder: Box<dyn CameraFinder>,
    ) -> Self {
        Self {
            known: Known::Found {
                finder,
                last: Mutex::new(Vec::new()),
            },
            settings: None,
            preview: None,
            camera: Mutex::new(camera),
        }
    }

    /// This slot with access to the camera's exposure settings (ISO, shutter speed, aperture).
    #[must_use]
    pub fn with_settings(mut self, settings: Box<dyn CameraSettings>) -> Self {
        self.settings = Some(settings);
        self
    }

    /// This slot with access to the camera's live view.
    #[must_use]
    pub fn with_preview(mut self, preview: Box<dyn CameraPreview>) -> Self {
        self.preview = Some(preview);
        self
    }

    fn preview_access(&self) -> Result<&dyn CameraPreview, AppError> {
        self.preview.as_deref().ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "This camera's live view is not available from the app.",
            )
        })
    }

    fn settings_access(&self) -> Result<&dyn CameraSettings, AppError> {
        self.settings.as_deref().ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "This camera's exposure settings cannot be read or changed from the app.",
            )
        })
    }

    /// The cameras there are now. A backend that cannot answer (a picture is being taken, or the
    /// camera gave up) gives the last list, and the reason is logged; the capture commands report
    /// it in words when they need the camera.
    pub fn list(&self) -> Vec<CameraInfo> {
        match &self.known {
            Known::Fixed(info) => vec![info.clone()],
            Known::Found { finder, last } => {
                let mut last = last.lock().unwrap_or_else(PoisonError::into_inner);
                match finder.list() {
                    Ok(cameras) => *last = cameras,
                    Err(err) => tracing::warn!("the camera list could not be refreshed: {err}"),
                }
                last.clone()
            }
        }
    }

    /// Makes sure the camera is found and open before a job takes it. A camera that is always
    /// there needs nothing; otherwise the error says in words what is wrong (none plugged in,
    /// macOS holding it, a camera that stopped answering).
    fn connect(&self) -> Result<(), AppError> {
        match &self.known {
            Known::Fixed(_) => Ok(()),
            Known::Found { finder, last } => {
                let info = finder.connect()?;
                let mut last = last.lock().unwrap_or_else(PoisonError::into_inner);
                if !last.contains(&info) {
                    *last = vec![info];
                }
                Ok(())
            }
        }
    }

    /// The camera for one job; never waits (the busy flag already serialises the jobs).
    fn take(&self) -> Result<MutexGuard<'_, Box<dyn CameraSource + Send>>, AppError> {
        match self.camera.try_lock() {
            Ok(camera) => Ok(camera),
            Err(TryLockError::Poisoned(poisoned)) => Ok(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => Err(busy()),
        }
    }
}

/// `camera_settings`: the camera's ISO, shutter speed and aperture with the choices it allows. A
/// setting the camera does not have comes back absent, not as an error. Refused while a capture or
/// a test shot runs (the camera is in use).
pub fn camera_settings(hardware: &Hardware) -> Result<Vec<CameraSetting>, AppError> {
    let camera = hardware.camera.clone().ok_or_else(no_camera)?;
    let access = camera.settings_access()?;
    let _busy = hardware.busy().acquire()?;
    camera.connect()?;
    access.settings()
}

/// `camera_set_setting`: sets one of the three settings to `value`, which must be one of the
/// camera's choices, and returns the settings as the camera has them afterwards. Refused while a
/// capture or a test shot runs.
pub fn camera_set_setting(
    hardware: &Hardware,
    kind: CameraSettingKind,
    value: &str,
) -> Result<Vec<CameraSetting>, AppError> {
    let camera = hardware.camera.clone().ok_or_else(no_camera)?;
    let access = camera.settings_access()?;
    let value = clean_value(value)?;
    let _busy = hardware.busy().acquire()?;
    camera.connect()?;
    access.set_setting(kind, &value)
}

/// `camera_preview_frame`: one live-view frame of the camera as JPEG bytes; the first one puts the
/// camera in live view. The light is never touched: the view shows what the controls have lit.
/// Refused while a capture, a test shot or a calibration runs. It only looks at the busy flag and
/// does not take it, so a frame never makes a light command or a capture start fail with "in
/// progress" (the view asks about 30 times a second); a capture that starts between two frames
/// leaves live view itself, and a frame asked for during it is refused by the camera backend.
pub fn camera_preview_frame(hardware: &Hardware) -> Result<Vec<u8>, AppError> {
    let camera = hardware.camera.clone().ok_or_else(no_camera)?;
    let access = camera.preview_access()?;
    if hardware.busy().is_busy() {
        return Err(busy());
    }
    access.preview_frame()
}

/// `camera_preview_stop`: leaves live view. Does nothing when the camera is not in live view, and
/// succeeds without doing anything while a capture, a test shot or a calibration runs: those leave
/// live view themselves before they take a picture, so there is nothing left to stop.
pub fn camera_preview_stop(hardware: &Hardware) -> Result<(), AppError> {
    let camera = hardware.camera.clone().ok_or_else(no_camera)?;
    let access = camera.preview_access()?;
    if hardware.busy().is_busy() {
        return Ok(());
    }
    access.preview_stop()
}

/// `camera_preview_zoom_steps` (H-18): the zoom steps of the camera's live view, whole frame
/// first; empty when the camera has no zoom. Like a frame it only looks at the busy flag.
pub fn camera_preview_zoom_steps(hardware: &Hardware) -> Result<Vec<ZoomStep>, AppError> {
    let camera = hardware.camera.clone().ok_or_else(no_camera)?;
    let access = camera.preview_access()?;
    if hardware.busy().is_busy() {
        return Err(busy());
    }
    access.preview_zoom_steps()
}

/// `camera_preview_set_zoom` (H-18): sets the live-view zoom to one of the steps (anything else is
/// refused); puts the camera in live view if it is not in it. The zoom goes back to the whole frame
/// when live view is left. Refused while a capture runs; it does not take the busy flag.
pub fn camera_preview_set_zoom(hardware: &Hardware, value: &str) -> Result<(), AppError> {
    let camera = hardware.camera.clone().ok_or_else(no_camera)?;
    let access = camera.preview_access()?;
    if hardware.busy().is_busy() {
        return Err(busy());
    }
    access.set_preview_zoom(value)
}

/// `camera_preview_set_area` (H-18): moves the zoomed area to the point (`x`, `y`) of the frame, as
/// fractions from 0 to 1. Refused while a capture runs; it does not take the busy flag.
pub fn camera_preview_set_area(hardware: &Hardware, x: f32, y: f32) -> Result<(), AppError> {
    let camera = hardware.camera.clone().ok_or_else(no_camera)?;
    let access = camera.preview_access()?;
    if hardware.busy().is_busy() {
        return Err(busy());
    }
    access.set_preview_area(x, y)
}

/// The error when no camera backend exists (a build without the macOS `tether` feature, and not
/// `GTP_FAKE_HW=1`, until the watched-folder backend of H-10).
fn no_camera() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "No camera backend is available in this version of the app, so a picture cannot be taken. The light can still be used on its own.",
    )
}

// --- light and camera commands ---------------------------------------------------------------

/// `light_list`: the serial ports the light may be on.
pub fn light_list(hardware: &Hardware) -> Result<Vec<PortInfo>, AppError> {
    hardware.light.list_ports()
}

/// `light_connect`: finds the light (on `port`, or by probing the serial ports). Refused while a
/// capture is in progress. The light is switched off first, and the watchdog's trip is cleared.
pub fn light_connect(hardware: &Hardware, port: Option<&str>) -> Result<LightInfo, AppError> {
    let port = port.map(str::trim).filter(|p| !p.is_empty());
    if let Some(port) = port {
        if port.chars().count() > MAX_PORT_CHARS || port.chars().any(char::is_control) {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "That is not a valid port name.",
            ));
        }
    }
    let _busy = hardware.busy().acquire()?;
    hardware.light.clone().connect(port)
}

/// `light_set`: lights red, green and blue at these levels. Refused while a capture is in
/// progress and after a temperature trip, and (for non-zero levels) when the light is not connected,
/// reports no current temperature or is over `abort_c`.
pub fn light_set(hardware: &Hardware, levels: [u8; 3], abort_c: f32) -> Result<(), AppError> {
    light_set_waiting(hardware, levels, abort_c, READING_WAIT)
}

/// [`light_set`] with the wait for a first reading given (tests shorten it). Non-zero levels need
/// a light that is connected, reports a current temperature and is not over `abort_c`, so a light
/// that cannot be monitored is never left on; all zeros are always allowed.
fn light_set_waiting(
    hardware: &Hardware,
    levels: [u8; 3],
    abort_c: f32,
    reading_wait: Duration,
) -> Result<(), AppError> {
    let _busy = hardware.busy().acquire()?;
    if levels != [0; 3] {
        wait_for_reading(&hardware.light, reading_wait);
        check_light(hardware.light.status(), abort_c)?;
    }
    hardware.light.clone().set_levels(levels)
}

/// `light_off`: switches the light off. Always allowed.
pub fn light_off(hardware: &Hardware) -> Result<(), AppError> {
    hardware.light.off()
}

/// `camera_list`: the cameras that can be used. With `GTP_FAKE_HW=1` the fake one; on macOS with
/// the `tether` feature the cameras libgphoto2 sees now (so one plugged in after the app started
/// shows up, and asking again also revives a camera that stopped answering); otherwise none.
pub fn camera_list(hardware: &Hardware) -> Vec<CameraInfo> {
    hardware
        .camera
        .as_ref()
        .map(|slot| slot.list())
        .unwrap_or_default()
}

impl Hardware {
    /// One tick of the temperature watchdog: reads the light, switches it off above `abort_c` and
    /// tells `events` what it read. Called every 200 ms by the thread that owns the status event.
    pub fn watch_light(&self, abort_c: f32, events: &dyn CaptureEvents) {
        if let Some(reading) = self.light.poll(abort_c) {
            events.light_status(&reading);
        }
    }
}

// --- steps of the camera ---------------------------------------------------------------------

/// A camera that reports each step of the channel in turn to `events`, and fails a picture that
/// arrives after the light was tripped (it may not have been lit for the whole exposure).
struct StepCamera<'a> {
    inner: &'a mut dyn CameraSource,
    events: &'a dyn CaptureEvents,
    channels: &'a [TripletChannel],
    light: &'a SharedLight,
    next: usize,
}

impl StepCamera<'_> {
    fn emit(&self, state: StepState) {
        if let Some(channel) = self.channels.get(self.next) {
            self.events.step(*channel, state);
        }
    }
}

impl CameraSource for StepCamera<'_> {
    fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    fn arm(&mut self) -> Result<(), AppError> {
        self.emit(StepState::Lit);
        self.inner.arm()
    }

    fn trigger(&mut self) -> Result<(), AppError> {
        self.inner.trigger()?;
        self.emit(StepState::Fired);
        Ok(())
    }

    fn wait_for_image(&mut self, timeout: Duration) -> Result<CapturedFile, AppError> {
        let file = self.inner.wait_for_image(timeout)?;
        if self.light.is_tripped() {
            return Err(tripped_error());
        }
        self.emit(StepState::Received);
        self.next += 1;
        Ok(file)
    }
}

// --- the capture job -------------------------------------------------------------------------

/// `capture_triplet`: starts capturing one frame into `roll` as a job and returns its id. The new
/// frame arrives in `job://done` like an import's. Refused when there is no camera backend, no
/// library or roll, or another capture is running. `levels` are the working levels of the sliders
/// (0 to 255 each), used instead of the profile's: they are recorded with the frame as the roll's
/// saved levels when they equal them, and as manual otherwise.
pub fn start_triplet(
    ctx: &AppContext,
    jobs: &JobManager,
    hardware: &Hardware,
    library_events: Arc<dyn LibraryEvents>,
    events: Arc<dyn CaptureEvents>,
    roll: RollId,
    levels: [u32; 3],
) -> Result<JobId, AppError> {
    let levels = levels_from_wide(levels)?;
    let camera = hardware.camera.clone().ok_or_else(no_camera)?;
    let library = ctx.library()?;
    // Fail now, not inside the job, when the roll is gone or the library is read-only.
    library.read(|c| c.get_roll(roll))?;
    library.write(|_| Ok(()))?;
    let busy = hardware.busy().acquire()?;
    let used = LevelsUsed::for_working(levels, &levels_for_roll(ctx, roll)?);
    let profile = CaptureProfile {
        levels,
        ..ctx.settings().capture_profile()
    };
    let look = ctx.settings().get().starting_look;
    let thumbnails = ctx.thumbnail_requester()?;
    let light = hardware.light.clone();
    let decoder = Arc::clone(&hardware.decoder);

    jobs.spawn(JobKind::Capture, move |job| {
        // Held until the job ends, also by a panic.
        let _busy = busy;
        let sequence = CaptureSequence::new(profile);
        // A camera plugged in after the app started is found here; an absent or held one fails
        // the job with words, before any light is lit.
        camera.connect()?;
        let mut cam = camera.take()?;
        let mut stepped = StepCamera {
            inner: &mut **cam,
            events: events.as_ref(),
            channels: &TripletChannel::ALL,
            light: &light,
            next: 0,
        };
        let mut handle = light.clone();
        let result = capture_triplet(
            &library,
            roll,
            &sequence,
            &mut handle,
            &mut stepped,
            decoder.as_ref(),
            job,
            library_events.as_ref(),
            look,
            &used,
        )?;
        // A better film base for the roll changes how its other frames look: refresh their thumbnails.
        if result.base_replaced {
            let following = roll_frame::frames_following_roll_base(&library, roll)?;
            thumbnails.request(Arc::clone(&library_events), following);
        }
        thumbnails.request(Arc::clone(&library_events), result.rebalanced.clone());
        tracing::info!(
            frames = result.frames.len(),
            warnings = result.warnings.len(),
            "capture finished"
        );
        Ok(JobResult::Imported(result))
    })
}

// --- the calibration job ---------------------------------------------------------------------

/// `capture_calibrate`: starts the automatic level calibration as a job and returns its id. `levels`
/// are the working levels (0 to 255 each) it starts from, `region` the part of the frame it measures
/// (the whole frame when `None`). It takes test shots, one colour at a time, until the brightest
/// pixels of each sit just below clipping (see `gtp_capture::calibration`), and ends with
/// `JobResult::Calibrated`. Nothing is saved or changed: the levels are the user's to review and
/// save. Same preconditions as [`start_triplet`]; the busy flag is held until the job ends, and the
/// light is off after every shot and on every way out.
pub fn start_calibration(
    ctx: &AppContext,
    jobs: &JobManager,
    hardware: &Hardware,
    events: Arc<dyn CaptureEvents>,
    roll: RollId,
    levels: [u32; 3],
    region: Option<Region>,
) -> Result<JobId, AppError> {
    let levels = levels_from_wide(levels)?;
    let region = region.map(Region::validated).transpose()?;
    let camera = hardware.camera.clone().ok_or_else(no_camera)?;
    // Fail now, not inside the job, when there is no library or the roll is gone.
    ctx.library()?.read(|c| c.get_roll(roll))?;
    let busy = hardware.busy().acquire()?;
    let profile = ctx.settings().capture_profile();
    let light = hardware.light.clone();
    let decoder = Arc::clone(&hardware.decoder);

    jobs.spawn(JobKind::Calibration, move |job| {
        // Held until the job ends, also by a panic.
        let _busy = busy;
        camera.connect()?;
        let mut cam = camera.take()?;
        let mut meter = SceneMeter {
            profile,
            light: &light,
            camera: &mut **cam,
            events: events.as_ref(),
            decoder: decoder.as_ref(),
            region,
            job,
            shots: 0,
            exposure: ExposureInfo::default(),
        };
        let mut result = calibrate(&mut meter, levels)?;
        result.exposure = meter.exposure;
        tracing::info!(
            levels = ?result.levels,
            shots = ?result.shots,
            warnings = result.warnings.len(),
            "calibration finished"
        );
        Ok(JobResult::Calibrated(result))
    })
}

/// Takes the shots of a calibration: lights one colour at the level asked for, shoots, and
/// measures that colour over the region.
struct SceneMeter<'a> {
    profile: CaptureProfile,
    light: &'a SharedLight,
    camera: &'a mut dyn CameraSource,
    events: &'a dyn CaptureEvents,
    decoder: &'a dyn RawDecoder,
    region: Option<Region>,
    job: &'a JobCtx,
    /// Shots taken so far, over all colours (for the progress fraction).
    shots: u32,
    /// The camera settings of the last shot, from its EXIF.
    exposure: ExposureInfo,
}

/// The most shots a calibration can take: the limit per colour for each of the three.
const MAX_SHOTS: f32 = 3.0 * gtp_capture::calibration::MAX_SHOTS_PER_CHANNEL as f32;

impl ChannelMeter for SceneMeter<'_> {
    fn measure(&mut self, channel: TripletChannel, level: u8, shot: u8) -> Result<f32, AppError> {
        let name = ["Red", "Green", "Blue"][channel.index()];
        self.job.progress(
            &format!("{name}: shot {shot}, level {level}"),
            self.shots as f32 / MAX_SHOTS,
        );
        self.shots += 1;
        let mut profile = self.profile.clone();
        profile.levels[channel.index()] = level;
        let file = shoot(&profile, channel, self.light, self.camera, self.events)?;
        let (staged, plane) = decode_plane(&file, channel, self.decoder)?;
        self.exposure = exposure_from_exif(read_exposure_info(&staged.path));
        let mut area = region_values(&plane, self.region);
        percentile(&mut area, PERCENTILE)
            .ok_or_else(|| AppError::new(ErrorCode::Io, "The picture has no pixels to measure."))
    }

    fn cancelled(&self) -> bool {
        self.job.is_cancelled()
    }
}

/// The values of a one-channel image inside `region` (all of it when `None`).
fn region_values(plane: &ImageF32, region: Option<Region>) -> Vec<f32> {
    let Some(region) = region else {
        return plane.data.clone();
    };
    let width = plane.width as usize;
    let (x0, y0, x1, y1) = region.pixels(plane.width, plane.height);
    let mut values = Vec::with_capacity((x1 - x0) * (y1 - y0));
    for row in y0..y1 {
        values.extend_from_slice(&plane.data[row * width + x0..row * width + x1]);
    }
    values
}

// --- the test shot ---------------------------------------------------------------------------

/// `capture_test`: lights `channel` alone at its level of `levels` (the working levels of the
/// sliders, 0 to 255 each), takes one picture, measures that channel and returns its histogram,
/// clipped share and the camera settings of the picture. Stores nothing, and the light is off
/// afterwards on every way out (the sequence's guard). Refused while a capture is in progress.
pub fn test_shot(
    ctx: &AppContext,
    hardware: &Hardware,
    events: &dyn CaptureEvents,
    channel: TripletChannel,
    levels: [u32; 3],
) -> Result<TestShot, AppError> {
    let levels = levels_from_wide(levels)?;
    let camera = hardware.camera.clone().ok_or_else(no_camera)?;
    let _busy = hardware.busy().acquire()?;
    let profile = CaptureProfile {
        levels,
        ..ctx.settings().capture_profile()
    };
    let file = {
        camera.connect()?;
        let mut cam = camera.take()?;
        shoot(&profile, channel, &hardware.light, &mut **cam, events)?
    };
    measure(&file, channel, hardware.decoder.as_ref())
}

/// Waits up to `wait` for the light's first temperature reading (it sends one every 200 ms). No
/// lock is held while waiting.
fn wait_for_reading(light: &SharedLight, wait: Duration) {
    let mut waited = Duration::ZERO;
    while waited < wait && light.status().temp_c.is_none() && light.status().connected {
        ThreadPause.pause(READING_SLICE);
        waited += READING_SLICE;
    }
}

/// The safety checks of a test shot, the sequence's: a light that is connected, reports a
/// temperature and is not over the abort limit.
fn check_light(status: LightStatus, abort_c: f32) -> Result<(), AppError> {
    if !status.connected {
        return Err(AppError::new(
            ErrorCode::Io,
            "The light is not connected. Connect it first.",
        ));
    }
    let Some(temp_c) = status.temp_c.filter(|t| t.is_finite()) else {
        return Err(AppError::new(
            ErrorCode::Io,
            "The light is not reporting its temperature, so it cannot be used safely. Reconnect it and try again.",
        ));
    };
    if temp_c > abort_c {
        return Err(AppError::new(
            ErrorCode::Io,
            format!(
                "The light is too hot ({temp_c:.0} C, the limit is {abort_c:.0} C). Let it cool down, then try again."
            ),
        ));
    }
    Ok(())
}

/// Lights one channel and takes the picture; the light is off when this returns or unwinds.
fn shoot(
    profile: &gtp_capture::CaptureProfile,
    channel: TripletChannel,
    light: &SharedLight,
    camera: &mut dyn CameraSource,
    events: &dyn CaptureEvents,
) -> Result<CapturedFile, AppError> {
    if !camera.is_connected() {
        return Err(AppError::new(
            ErrorCode::Io,
            "The camera is not connected. Check its USB cable and that it is switched on.",
        ));
    }
    wait_for_reading(light, READING_WAIT);
    check_light(light.status(), profile.temp_abort_c)?;

    let mut handle = light.clone();
    let guard = LightGuard::new(&mut handle);
    let mut levels = [0u8; 3];
    levels[channel.index()] = profile.levels[channel.index()];
    guard.light.set_levels(levels)?;
    ThreadPause.pause(Duration::from_millis(u64::from(profile.settle_ms)));
    check_light(guard.light.status(), profile.temp_abort_c)?;

    let channels = [channel];
    let mut stepped = StepCamera {
        inner: camera,
        events,
        channels: &channels,
        light,
        next: 0,
    };
    stepped.arm()?;
    stepped.trigger()?;
    stepped.wait_for_image(CAPTURE_TIMEOUT)
}

/// A picture written to a temporary file for the decoder, removed when dropped.
struct TempPicture {
    dir: PathBuf,
    path: PathBuf,
}

impl TempPicture {
    fn write(channel: TripletChannel, file: &CapturedFile) -> Result<Self, AppError> {
        let dir = std::env::temp_dir().join(format!("gtp-capture-test-{}", FrameId::new()));
        std::fs::create_dir_all(&dir).map_err(|err| AppError::from_io(&err))?;
        let prefix = ["r", "g", "b"][channel.index()];
        let path = dir.join(format!("{prefix}-{}", safe_name(&file.name)));
        let picture = Self { dir, path };
        std::fs::write(&picture.path, &file.bytes).map_err(|err| AppError::from_io(&err))?;
        Ok(picture)
    }
}

impl Drop for TempPicture {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_dir_all(&self.dir) {
            tracing::debug!("removing the test shot folder failed: {err}");
        }
    }
}

/// Stages `file` and decodes it to the lit channel's plane. The staged file lives as long as the
/// returned guard.
fn decode_plane(
    file: &CapturedFile,
    channel: TripletChannel,
    decoder: &dyn RawDecoder,
) -> Result<(TempPicture, ImageF32), AppError> {
    let staged = TempPicture::write(channel, file)?;
    let raw = decoder.decode(&staged.path, &Limits::default())?;
    let plane = demosaic_channel(&raw, channel.index())?;
    Ok((staged, plane))
}

/// The histogram and clipped share of the lit channel of `file`.
fn measure(
    file: &CapturedFile,
    channel: TripletChannel,
    decoder: &dyn RawDecoder,
) -> Result<TestShot, AppError> {
    let (staged, plane) = decode_plane(file, channel, decoder)?;
    let mut histogram = vec![0u32; 256];
    let mut clipped = 0usize;
    for value in &plane.data {
        let value = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        histogram[(value * 255.0).round() as usize] += 1;
        if value >= CLIP_LEVEL {
            clipped += 1;
        }
    }
    let clipped_pct = if plane.data.is_empty() {
        0.0
    } else {
        100.0 * clipped as f32 / plane.data.len() as f32
    };
    Ok(TestShot {
        histogram,
        clipped_pct,
        exposure: exposure_from_exif(read_exposure_info(&staged.path)),
    })
}

#[cfg(test)]
#[path = "capture_control_tests.rs"]
mod tests;
