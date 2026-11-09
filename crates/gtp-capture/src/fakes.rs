//! A light for tests that records what it is told. Always compiled and public, like
//! `gtp_ingest::FixtureSource`, so the capture sequence and the application layer can test
//! against it without hardware.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use gtp_core::{AppError, ErrorCode};

use crate::camera::{CameraSource, CapturedFile};
use crate::light::{LightInfo, LightSource, LightStatus};
use crate::preview::{check_area, check_zoom, zoom_steps_from_choices, CameraPreview, ZoomStep};
use crate::settings::{check_change, CameraSetting, CameraSettingKind, CameraSettings};

/// A command a [`FakeLight`] received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LightCommand {
    /// `connect`, with the port asked for.
    Connect(Option<String>),
    /// `set_levels`. The dropped light records `SetLevels([0, 0, 0])` last, as the real driver
    /// sends a switch-off when it closes.
    SetLevels([u8; 3]),
    /// `shutter_pulse`, in units of 10 ms.
    ShutterPulse(u8),
}

#[derive(Default)]
struct State {
    commands: Vec<LightCommand>,
    status: LightStatus,
    fail: Option<AppError>,
}

/// A handle on the commands and the status of a [`FakeLight`], still usable after the light has
/// been moved into a `Box<dyn LightSource>` or dropped.
#[derive(Clone, Default)]
pub struct FakeLightHandle(Arc<Mutex<State>>);

impl FakeLightHandle {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every command so far, in order.
    pub fn commands(&self) -> Vec<LightCommand> {
        self.state().commands.clone()
    }

    /// The levels last set (all zeros if none, or after a switch-off).
    pub fn levels(&self) -> [u8; 3] {
        self.state()
            .commands
            .iter()
            .rev()
            .find_map(|command| match command {
                LightCommand::SetLevels(levels) => Some(*levels),
                _ => None,
            })
            .unwrap_or([0; 3])
    }

    /// The level the light last had for colour `index` (0 red, 1 green, 2 blue) while that colour
    /// was lit, which survives the switch-off after a shot; 0 if it never was lit.
    pub fn last_lit_level(&self, index: usize) -> u8 {
        self.state()
            .commands
            .iter()
            .rev()
            .find_map(|command| match command {
                LightCommand::SetLevels(levels) if levels.get(index).is_some_and(|l| *l > 0) => {
                    Some(levels[index])
                }
                _ => None,
            })
            .unwrap_or(0)
    }

    /// What `status()` returns from now on (for temperature tests).
    pub fn set_status(&self, status: LightStatus) {
        self.state().status = status;
    }

    /// Makes `set_levels` and `shutter_pulse` fail with this error until cleared with `None`.
    /// The failed command is not recorded.
    pub fn fail_commands_with(&self, error: Option<AppError>) {
        self.state().fail = error;
    }
}

/// A light that records its commands. Dropping it records a switch-off, like the real driver.
pub struct FakeLight {
    handle: FakeLightHandle,
    info: LightInfo,
}

impl Default for FakeLight {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeLight {
    /// A connected-looking V4 rev A (hardware ID 1) at 25 °C.
    pub fn new() -> Self {
        let handle = FakeLightHandle::default();
        handle.set_status(LightStatus {
            temp_c: Some(25.0),
            vbus_mv: Some(5150),
            connected: true,
        });
        Self {
            handle,
            info: LightInfo {
                port: "fake".to_owned(),
                hardware_id: 1,
                firmware_id: 0,
                recognised: true,
            },
        }
    }

    /// A handle to read the commands from.
    pub fn handle(&self) -> FakeLightHandle {
        self.handle.clone()
    }
}

impl LightSource for FakeLight {
    fn connect(&mut self, port: Option<&str>) -> Result<LightInfo, AppError> {
        self.handle
            .state()
            .commands
            .push(LightCommand::Connect(port.map(str::to_owned)));
        Ok(self.info.clone())
    }

    fn set_levels(&mut self, rgb: [u8; 3]) -> Result<(), AppError> {
        self.record(LightCommand::SetLevels(rgb))
    }

    fn shutter_pulse(&mut self, tens_of_ms: u8) -> Result<(), AppError> {
        self.record(LightCommand::ShutterPulse(tens_of_ms))
    }

    fn status(&self) -> LightStatus {
        self.handle.state().status
    }
}

impl FakeLight {
    fn record(&mut self, command: LightCommand) -> Result<(), AppError> {
        let mut state = self.handle.state();
        if let Some(error) = &state.fail {
            return Err(error.clone());
        }
        state.commands.push(command);
        Ok(())
    }
}

impl Drop for FakeLight {
    fn drop(&mut self) {
        self.handle
            .state()
            .commands
            .push(LightCommand::SetLevels([0; 3]));
    }
}

/// The error a failing fake gives when none is chosen.
pub fn unplugged() -> AppError {
    AppError::new(ErrorCode::Io, "The light is not connected.")
}

/// What a [`FakeCamera`] does at one `wait_for_image`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shot {
    /// Delivers this picture.
    File(CapturedFile),
    /// Fails with this error (a timeout, a camera error).
    Fail(AppError),
}

/// A call a [`FakeCamera`] received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraCall {
    /// `arm`.
    Arm,
    /// `trigger`, with the light levels at that moment when the camera is linked to a light.
    Trigger { levels: Option<[u8; 3]> },
    /// `wait_for_image`, with the timeout asked for.
    Wait(Duration),
}

type WaitHook = Box<dyn FnMut(usize) + Send>;

/// How the fake's live view is set when a frame is made: the zoom step and the zoomed area.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewView {
    /// The zoom choice now (`"Entire Display"` until one is set).
    pub zoom: String,
    /// The point of the frame last given to `set_preview_area`, as fractions; `None` until then.
    pub area: Option<(f32, f32)>,
}

/// The choices of the fake's live-view zoom, whole frame first.
pub const FAKE_ZOOM_CHOICES: [&str; 5] = ["Entire Display", "25%", "50%", "100%", "200%"];

/// Makes live-view frame number `n` (from 0) for the view the camera is set to.
type FrameMaker = Arc<dyn Fn(usize, &PreviewView) -> Result<Vec<u8>, AppError> + Send + Sync>;

/// The frame a fake camera serves unless told how to make them: the JPEG start and end markers
/// around the frame's number, so that frames differ. It is not a picture; a fake that has to show
/// something (`GTP_FAKE_HW=1`) is given a maker of real JPEGs by [`FakeCamera::with_frames`].
fn placeholder_frame(n: usize) -> Result<Vec<u8>, AppError> {
    let mut frame = vec![0xFF, 0xD8];
    frame.extend_from_slice(&n.to_le_bytes());
    frame.extend_from_slice(&[0xFF, 0xD9]);
    Ok(frame)
}

#[derive(Default)]
struct CameraState {
    shots: VecDeque<Shot>,
    repeat: Option<CapturedFile>,
    calls: Vec<CameraCall>,
    connected: bool,
    fail_arm: Option<AppError>,
    fail_trigger: Option<AppError>,
    light: Option<FakeLightHandle>,
    waits: usize,
    on_wait: Option<WaitHook>,
    settings: Vec<CameraSetting>,
    fail_settings: Option<AppError>,
    changes: Vec<(CameraSettingKind, String)>,
    frames: usize,
    frame_maker: Option<FrameMaker>,
    fail_preview: Option<AppError>,
    preview_stops: usize,
    live_view: bool,
    zoom: String,
    area: Option<(f32, f32)>,
    prohibit: Option<String>,
}

impl CameraState {
    fn view(&self) -> PreviewView {
        PreviewView {
            zoom: self.zoom.clone(),
            area: self.area,
        }
    }

    fn zoom_steps() -> Vec<ZoomStep> {
        zoom_steps_from_choices(FAKE_ZOOM_CHOICES.map(str::to_owned))
    }

    /// What leaving live view does to the zoom, as on the camera: back to the whole frame.
    fn leave_live_view(&mut self) {
        self.live_view = false;
        FAKE_ZOOM_CHOICES[0].clone_into(&mut self.zoom);
    }
}

/// What a fake camera offers until a test changes it: ISO 100, 1/100 s and f/8, all writable.
fn default_settings() -> Vec<CameraSetting> {
    let list = |values: &[&str]| values.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
    vec![
        CameraSetting::from_camera(
            CameraSettingKind::Iso,
            "100",
            list(&["100", "200", "400", "800"]),
            true,
        ),
        CameraSetting::from_camera(
            CameraSettingKind::ShutterSpeed,
            "1/100",
            list(&["1/60", "1/100", "1/125", "1/250"]),
            true,
        ),
        CameraSetting::from_camera(
            CameraSettingKind::Aperture,
            "8",
            list(&["5.6", "8", "11"]),
            true,
        ),
    ]
}

/// A handle on a [`FakeCamera`], still usable after the camera was moved into a `Box`.
#[derive(Clone, Default)]
pub struct FakeCameraHandle(Arc<Mutex<CameraState>>);

impl FakeCameraHandle {
    fn state(&self) -> MutexGuard<'_, CameraState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every call so far, in order.
    pub fn calls(&self) -> Vec<CameraCall> {
        self.state().calls.clone()
    }

    /// What `is_connected` returns from now on.
    pub fn set_connected(&self, connected: bool) {
        self.state().connected = connected;
    }

    /// Makes `arm` fail until cleared with `None`.
    pub fn fail_arm_with(&self, error: Option<AppError>) {
        self.state().fail_arm = error;
    }

    /// Makes `trigger` fail until cleared with `None`.
    pub fn fail_trigger_with(&self, error: Option<AppError>) {
        self.state().fail_trigger = error;
    }

    /// Makes the camera lack `kind` (a lens without electronic contacts has no aperture).
    pub fn make_setting_absent(&self, kind: CameraSettingKind) {
        self.replace_setting(CameraSetting::absent(kind));
    }

    /// Makes `kind` readonly (shutter speed and aperture outside M mode), keeping its choices.
    pub fn make_setting_readonly(&self, kind: CameraSettingKind) {
        if let Some(s) = self.state().settings.iter_mut().find(|s| s.kind == kind) {
            s.writable = false;
        }
    }

    /// Makes reading and changing the settings fail until cleared with `None`.
    pub fn fail_settings_with(&self, error: Option<AppError>) {
        self.state().fail_settings = error;
    }

    /// Every change made so far through [`CameraSettings::set_setting`], in order.
    pub fn changes(&self) -> Vec<(CameraSettingKind, String)> {
        self.state().changes.clone()
    }

    /// Live-view frames served so far.
    pub fn preview_frames(&self) -> usize {
        self.state().frames
    }

    /// Calls of [`CameraPreview::preview_stop`] so far, whether or not live view was on.
    pub fn preview_stops(&self) -> usize {
        self.state().preview_stops
    }

    /// True after a frame was served and until live view is left by a stop or by a capture.
    pub fn in_live_view(&self) -> bool {
        self.state().live_view
    }

    /// The live-view zoom now (`"Entire Display"` when unzoomed).
    pub fn preview_zoom(&self) -> String {
        self.state().zoom.clone()
    }

    /// The point of the frame last given to `set_preview_area`, as fractions (kept when live view
    /// is left, as on the camera).
    pub fn preview_area(&self) -> Option<(f32, f32)> {
        self.state().area
    }

    /// Makes `preview_prohibit` answer with this condition until cleared with `None`.
    pub fn set_prohibit(&self, reason: Option<&str>) {
        self.state().prohibit = reason.map(str::to_owned);
    }

    /// Makes frames and stops fail until cleared with `None`.
    pub fn fail_preview_with(&self, error: Option<AppError>) {
        self.state().fail_preview = error;
    }

    fn replace_setting(&self, setting: CameraSetting) {
        if let Some(s) = self
            .state()
            .settings
            .iter_mut()
            .find(|s| s.kind == setting.kind)
        {
            *s = setting;
        }
    }
}

impl CameraSettings for FakeCameraHandle {
    fn settings(&self) -> Result<Vec<CameraSetting>, AppError> {
        let state = self.state();
        state.fail_settings.clone().map_or(Ok(()), Err)?;
        Ok(state.settings.clone())
    }

    fn set_setting(
        &self,
        kind: CameraSettingKind,
        value: &str,
    ) -> Result<Vec<CameraSetting>, AppError> {
        let mut state = self.state();
        state.fail_settings.clone().map_or(Ok(()), Err)?;
        check_change(&state.settings, kind, value)?;
        if let Some(s) = state.settings.iter_mut().find(|s| s.kind == kind) {
            s.value = Some(value.to_owned());
        }
        state.changes.push((kind, value.to_owned()));
        Ok(state.settings.clone())
    }
}

impl CameraPreview for FakeCameraHandle {
    fn preview_frame(&self) -> Result<Vec<u8>, AppError> {
        let mut state = self.state();
        state.fail_preview.clone().map_or(Ok(()), Err)?;
        let n = state.frames;
        let frame = match &state.frame_maker {
            Some(maker) => maker(n, &state.view())?,
            None => placeholder_frame(n)?,
        };
        state.frames += 1;
        state.live_view = true;
        Ok(frame)
    }

    fn preview_stop(&self) -> Result<(), AppError> {
        let mut state = self.state();
        state.preview_stops += 1;
        state.fail_preview.clone().map_or(Ok(()), Err)?;
        state.leave_live_view();
        Ok(())
    }

    fn preview_zoom_steps(&self) -> Result<Vec<ZoomStep>, AppError> {
        self.state().fail_preview.clone().map_or(Ok(()), Err)?;
        Ok(CameraState::zoom_steps())
    }

    fn set_preview_zoom(&self, value: &str) -> Result<(), AppError> {
        let mut state = self.state();
        state.fail_preview.clone().map_or(Ok(()), Err)?;
        let value = check_zoom(&CameraState::zoom_steps(), value)?;
        state.live_view = true;
        state.zoom = value;
        Ok(())
    }

    fn set_preview_area(&self, x: f32, y: f32) -> Result<(), AppError> {
        check_area(x, y)?;
        let mut state = self.state();
        state.fail_preview.clone().map_or(Ok(()), Err)?;
        state.live_view = true;
        state.area = Some((x, y));
        Ok(())
    }

    fn preview_prohibit(&self) -> Result<Option<String>, AppError> {
        Ok(self.state().prohibit.clone())
    }
}

/// A camera that delivers scripted pictures and records its calls. With no shots left it fails
/// like a camera that never answered.
pub struct FakeCamera {
    handle: FakeCameraHandle,
}

impl FakeCamera {
    /// A connected camera that plays `shots`, one per `wait_for_image`.
    pub fn new(shots: Vec<Shot>) -> Self {
        let handle = FakeCameraHandle::default();
        {
            let mut state = handle.state();
            state.connected = true;
            state.shots = shots.into();
            state.settings = default_settings();
            FAKE_ZOOM_CHOICES[0].clone_into(&mut state.zoom);
        }
        Self { handle }
    }

    /// A connected camera that delivers `files`, one per shot.
    pub fn with_files(files: Vec<CapturedFile>) -> Self {
        Self::new(files.into_iter().map(Shot::File).collect())
    }

    /// When the scripted shots run out, delivers `file` for every further shot instead of failing
    /// (the camera of `GTP_FAKE_HW=1`, which has to serve any number of frames).
    #[must_use]
    pub fn repeating(self, file: CapturedFile) -> Self {
        self.handle.state().repeat = Some(file);
        self
    }

    /// Records the light's levels at each `trigger`.
    #[must_use]
    pub fn linked_to(self, light: FakeLightHandle) -> Self {
        self.handle.state().light = Some(light);
        self
    }

    /// Calls `hook` with the number of the shot (from 0) at the start of each `wait_for_image`,
    /// before its result is chosen: a test can change the light's status or fail its commands at
    /// exactly that point of the sequence. The hook must not call this camera's handle.
    #[must_use]
    pub fn on_wait(self, hook: impl FnMut(usize) + Send + 'static) -> Self {
        self.handle.state().on_wait = Some(Box::new(hook));
        self
    }

    /// Serves live-view frames made by `maker` (given the frame's number, from 0, and the zoom and
    /// area the camera is set to) instead of the placeholder ones (the camera of `GTP_FAKE_HW=1`
    /// serves real JPEGs).
    #[must_use]
    pub fn with_frames(
        self,
        maker: impl Fn(usize, &PreviewView) -> Result<Vec<u8>, AppError> + Send + Sync + 'static,
    ) -> Self {
        self.handle.state().frame_maker = Some(Arc::new(maker));
        self
    }

    /// A handle to read the calls from.
    pub fn handle(&self) -> FakeCameraHandle {
        self.handle.clone()
    }
}

impl CameraSource for FakeCamera {
    fn is_connected(&self) -> bool {
        self.handle.state().connected
    }

    fn arm(&mut self) -> Result<(), AppError> {
        let mut state = self.handle.state();
        state.calls.push(CameraCall::Arm);
        state.fail_arm.clone().map_or(Ok(()), Err)
    }

    fn trigger(&mut self) -> Result<(), AppError> {
        let mut state = self.handle.state();
        let levels = state.light.as_ref().map(FakeLightHandle::levels);
        state.calls.push(CameraCall::Trigger { levels });
        // A real camera is taken out of live view (and its zoom reset) before a capture.
        state.leave_live_view();
        state.fail_trigger.clone().map_or(Ok(()), Err)
    }

    fn wait_for_image(&mut self, timeout: Duration) -> Result<CapturedFile, AppError> {
        let (shot, hook) = {
            let mut state = self.handle.state();
            state.calls.push(CameraCall::Wait(timeout));
            let shot = state.waits;
            state.waits += 1;
            (shot, state.on_wait.take())
        };
        if let Some(mut hook) = hook {
            hook(shot);
            self.handle.state().on_wait = Some(hook);
        }
        let mut state = self.handle.state();
        match state.shots.pop_front() {
            Some(Shot::File(file)) => Ok(file),
            Some(Shot::Fail(error)) => Err(error),
            None => state.repeat.clone().ok_or_else(no_picture),
        }
    }
}

/// The error a camera gives when no picture came in time.
pub fn no_picture() -> AppError {
    AppError::new(
        ErrorCode::Io,
        "The camera did not deliver a picture in time.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_lit_level_survives_a_switch_off_per_colour() {
        let mut light = FakeLight::new();
        let handle = light.handle();
        assert_eq!([0, 1, 2].map(|i| handle.last_lit_level(i)), [0; 3]);
        light.set_levels([10, 0, 0]).unwrap();
        light.set_levels([0, 20, 0]).unwrap();
        light.set_levels([0; 3]).unwrap();
        assert_eq!([0, 1, 2].map(|i| handle.last_lit_level(i)), [10, 20, 0]);
        light.set_levels([0, 0, 7]).unwrap();
        light.set_levels([30, 0, 0]).unwrap();
        assert_eq!([0, 1, 2].map(|i| handle.last_lit_level(i)), [30, 20, 7]);
    }

    #[test]
    fn the_fake_records_commands_in_order_and_switches_off_when_dropped() {
        let mut light = FakeLight::new();
        let handle = light.handle();
        light.connect(Some("COM5")).unwrap();
        light.set_levels([44, 0, 0]).unwrap();
        assert_eq!(handle.levels(), [44, 0, 0]);
        light.shutter_pulse(10).unwrap();
        drop(light);
        assert_eq!(
            handle.commands(),
            [
                LightCommand::Connect(Some("COM5".into())),
                LightCommand::SetLevels([44, 0, 0]),
                LightCommand::ShutterPulse(10),
                LightCommand::SetLevels([0, 0, 0]),
            ]
        );
        assert_eq!(handle.levels(), [0; 3]);
    }

    #[test]
    fn a_failing_fake_returns_the_error_and_records_nothing() {
        let mut light = FakeLight::new();
        let handle = light.handle();
        handle.fail_commands_with(Some(unplugged()));
        assert!(light.set_levels([1, 2, 3]).is_err());
        assert!(handle.commands().is_empty());
        handle.fail_commands_with(None);
        assert!(light.set_levels([1, 2, 3]).is_ok());
    }

    #[test]
    fn a_repeating_camera_serves_any_number_of_shots_after_the_scripted_ones() {
        let file = |n: u8| CapturedFile {
            name: format!("{n}.nef"),
            bytes: vec![n],
        };
        let mut camera = FakeCamera::new(vec![Shot::File(file(1))]).repeating(file(9));
        let timeout = Duration::from_secs(1);
        assert_eq!(camera.wait_for_image(timeout).unwrap(), file(1));
        for _ in 0..3 {
            assert_eq!(camera.wait_for_image(timeout).unwrap(), file(9));
        }
        let mut empty = FakeCamera::new(vec![]);
        assert_eq!(empty.wait_for_image(timeout).unwrap_err(), no_picture());
    }

    #[test]
    fn the_fake_camera_offers_settings_and_changes_them_within_their_choices() {
        let camera = FakeCamera::new(vec![]);
        let handle = camera.handle();
        let first = handle.settings().unwrap();
        assert_eq!(first.len(), 3);
        assert_eq!(first[0].value.as_deref(), Some("100"));
        let after = handle.set_setting(CameraSettingKind::Iso, "400").unwrap();
        assert_eq!(after[0].value.as_deref(), Some("400"));
        assert_eq!(handle.settings().unwrap(), after);
        assert!(handle.set_setting(CameraSettingKind::Iso, "401").is_err());
        assert_eq!(
            handle.changes(),
            [(CameraSettingKind::Iso, "400".to_owned())]
        );

        handle.make_setting_readonly(CameraSettingKind::ShutterSpeed);
        assert!(handle
            .set_setting(CameraSettingKind::ShutterSpeed, "1/60")
            .is_err());
        handle.make_setting_absent(CameraSettingKind::Aperture);
        assert_eq!(
            handle.settings().unwrap()[2],
            CameraSetting::absent(CameraSettingKind::Aperture)
        );
        handle.fail_settings_with(Some(unplugged()));
        assert!(handle.settings().is_err());
        assert!(handle.set_setting(CameraSettingKind::Iso, "200").is_err());
        handle.fail_settings_with(None);
        assert!(handle.settings().is_ok());
    }

    #[test]
    fn the_status_can_be_scripted() {
        let light = FakeLight::new();
        light.handle().set_status(LightStatus {
            temp_c: Some(80.0),
            ..light.status()
        });
        assert_eq!(light.status().temp_c, Some(80.0));
    }

    #[test]
    fn live_view_frames_are_jpeg_markers_that_differ_and_a_capture_or_stop_leaves_live_view() {
        let mut camera = FakeCamera::new(vec![Shot::File(CapturedFile {
            name: "a.nef".to_owned(),
            bytes: vec![1],
        })]);
        let handle = camera.handle();
        assert!(!handle.in_live_view());
        let first = handle.preview_frame().unwrap();
        let second = handle.preview_frame().unwrap();
        for frame in [&first, &second] {
            assert_eq!(frame[..2], [0xFF, 0xD8]);
            assert_eq!(frame[frame.len() - 2..], [0xFF, 0xD9]);
        }
        assert_ne!(first, second);
        assert_eq!((handle.preview_frames(), handle.in_live_view()), (2, true));
        camera.arm().unwrap();
        camera.trigger().unwrap();
        assert!(!handle.in_live_view(), "a capture leaves live view");
        handle.preview_frame().unwrap();
        handle.preview_stop().unwrap();
        handle.preview_stop().unwrap();
        assert_eq!((handle.preview_stops(), handle.in_live_view()), (2, false));
    }

    #[test]
    fn the_fake_offers_zoom_steps_records_zoom_and_area_and_resets_the_zoom_on_leaving() {
        let mut camera = FakeCamera::new(vec![Shot::File(CapturedFile {
            name: "a.nef".to_owned(),
            bytes: vec![1],
        })])
        .with_frames(|_, view| Ok(format!("{}|{:?}", view.zoom, view.area).into_bytes()));
        let handle = camera.handle();
        let steps = handle.preview_zoom_steps().unwrap();
        let labels: Vec<&str> = steps.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["Whole frame", "25%", "50%", "100%", "200%"]);
        assert_eq!(steps[0].value, "Entire Display");

        assert_eq!(handle.preview_zoom(), "Entire Display");
        assert_eq!(handle.preview_area(), None);
        handle.set_preview_zoom("100%").unwrap();
        assert!(handle.in_live_view(), "the zoom starts live view");
        handle.set_preview_area(0.25, 0.75).unwrap();
        assert_eq!(handle.preview_zoom(), "100%");
        assert_eq!(handle.preview_area(), Some((0.25, 0.75)));
        assert_eq!(handle.preview_frame().unwrap(), b"100%|Some((0.25, 0.75))");

        assert!(handle.set_preview_zoom("300%").is_err());
        assert!(handle.set_preview_area(1.5, 0.5).is_err());
        assert!(handle.set_preview_area(f32::NAN, 0.5).is_err());
        assert_eq!(
            handle.preview_zoom(),
            "100%",
            "a refused value changes nothing"
        );
        assert_eq!(handle.preview_area(), Some((0.25, 0.75)));

        handle.preview_stop().unwrap();
        assert_eq!(handle.preview_zoom(), "Entire Display");
        assert_eq!(handle.preview_area(), Some((0.25, 0.75)), "the area stays");
        handle.set_preview_zoom("200%").unwrap();
        camera.arm().unwrap();
        camera.trigger().unwrap();
        assert_eq!(
            handle.preview_zoom(),
            "Entire Display",
            "a capture resets it too"
        );
        assert!(crate::preview::is_whole_frame(&handle.preview_zoom()));
    }

    #[test]
    fn the_prohibit_condition_and_failures_can_be_scripted() {
        let handle = FakeCamera::new(Vec::new()).handle();
        assert_eq!(handle.preview_prohibit().unwrap(), None);
        handle.set_prohibit(Some("Sequence error"));
        assert_eq!(
            handle.preview_prohibit().unwrap().as_deref(),
            Some("Sequence error")
        );
        handle.fail_preview_with(Some(unplugged()));
        assert!(handle.preview_zoom_steps().is_err());
        assert!(handle.set_preview_zoom("100%").is_err());
        assert!(handle.set_preview_area(0.5, 0.5).is_err());
    }

    #[test]
    fn frames_can_come_from_a_maker_and_the_preview_can_be_made_to_fail() {
        let camera = FakeCamera::new(Vec::new()).with_frames(|n, _| Ok(vec![n as u8]));
        let handle = camera.handle();
        assert_eq!(handle.preview_frame().unwrap(), [0]);
        assert_eq!(handle.preview_frame().unwrap(), [1]);
        handle.fail_preview_with(Some(unplugged()));
        assert!(handle.preview_frame().is_err());
        assert!(handle.preview_stop().is_err());
        assert_eq!(handle.preview_frames(), 2, "a failed frame is not counted");
    }
}
