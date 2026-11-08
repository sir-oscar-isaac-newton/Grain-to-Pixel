//! Tests of the capture commands on the fakes (see `capture_control.rs`).

use std::ffi::OsString;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc;

use gtp_capture::fakes::no_picture;
use gtp_capture::{
    CameraCall, CaptureProfile, CaptureProfilePatch, FakeCamera, FakeCameraHandle, FakeLight,
    FakeLightHandle, LightCommand, Shot,
};
use gtp_io::{Cfa, RawLinear};

use super::*;
use crate::fake_capture::{fake_camera, fake_camera_info, FakeRawDecoder, LevelledRawDecoder};
use crate::jobs::JobSink;
use crate::services::roll_frame::{roll_create, RollInput};
use crate::shared_light::fake_ports;

const WAIT: Duration = Duration::from_secs(30);

enum Outcome {
    Done(JobResult),
    Error(AppError),
}

struct Collect(Mutex<mpsc::Sender<Outcome>>);

impl JobSink for Collect {
    fn progress(&self, _: JobId, _: &str, _: f32) {}
    fn done(&self, _: JobId, result: JobResult) {
        let _ = self.0.lock().unwrap().send(Outcome::Done(result));
    }
    fn error(&self, _: JobId, error: AppError) {
        let _ = self.0.lock().unwrap().send(Outcome::Error(error));
    }
}

#[derive(Default)]
struct Recorder {
    steps: Mutex<Vec<(TripletChannel, StepState)>>,
    statuses: Mutex<Vec<LightReading>>,
}

impl CaptureEvents for Recorder {
    fn step(&self, channel: TripletChannel, state: StepState) {
        self.steps.lock().unwrap().push((channel, state));
    }
    fn light_status(&self, reading: &LightReading) {
        self.statuses.lock().unwrap().push(*reading);
    }
}

impl Recorder {
    fn steps(&self) -> Vec<(TripletChannel, StepState)> {
        self.steps.lock().unwrap().clone()
    }

    fn statuses(&self) -> Vec<LightReading> {
        self.statuses.lock().unwrap().clone()
    }
}

struct NoLibraryEvents;

impl LibraryEvents for NoLibraryEvents {
    fn library_changed(&self, _: Option<RollId>) {}
}

/// Reads the fake pictures, but the red-lit one is clipped at every red photosite.
struct ClipRed;

impl RawDecoder for ClipRed {
    fn decode(&self, path: &std::path::Path, limits: &Limits) -> Result<RawLinear, AppError> {
        let mut raw = FakeRawDecoder.decode(path, limits)?;
        let red = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('r'));
        if red {
            let width = raw.width as usize;
            for (i, value) in raw.data.iter_mut().enumerate() {
                if Cfa::RGGB.color_at(i % width, i / width) == 0 {
                    *value = 10_000;
                }
            }
        }
        Ok(raw)
    }
}

struct PanicCamera;

impl CameraSource for PanicCamera {
    fn is_connected(&self) -> bool {
        true
    }
    fn arm(&mut self) -> Result<(), AppError> {
        Ok(())
    }
    fn trigger(&mut self) -> Result<(), AppError> {
        Ok(())
    }
    fn wait_for_image(&mut self, _: Duration) -> Result<CapturedFile, AppError> {
        panic!("the camera driver crashed")
    }
}

/// The two ends of a camera that blocks inside its first `wait_for_image` until released.
struct Gate {
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}

fn gated(light: FakeLightHandle) -> (Box<dyn CameraSource + Send>, Gate) {
    let (entered_tx, entered) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let camera = fake_camera().linked_to(light).on_wait(move |shot| {
        if shot == 0 {
            let _ = entered_tx.send(());
            let _ = release_rx.recv_timeout(WAIT);
        }
    });
    (Box::new(camera), Gate { entered, release })
}

struct Rig {
    dir: PathBuf,
    ctx: Option<AppContext>,
    roll: RollId,
    hardware: Hardware,
    light: FakeLightHandle,
    jobs: JobManager,
    outcomes: mpsc::Receiver<Outcome>,
    events: Arc<Recorder>,
}

impl Rig {
    fn new(label: &str) -> Self {
        Self::build(label, Arc::new(FakeRawDecoder), |light| {
            Box::new(fake_camera().linked_to(light))
        })
    }

    fn build(
        label: &str,
        decoder: Arc<dyn RawDecoder>,
        camera: impl FnOnce(FakeLightHandle) -> Box<dyn CameraSource + Send>,
    ) -> Self {
        Self::build_with(label, |_| decoder, camera)
    }

    /// A rig whose decoder follows the fake light, as with `GTP_FAKE_HW=1`.
    fn levelled(label: &str) -> Self {
        Self::build_with(
            label,
            |light| Arc::new(LevelledRawDecoder::new(light)),
            |light| Box::new(fake_camera().linked_to(light)),
        )
    }

    fn build_with(
        label: &str,
        decoder: impl FnOnce(FakeLightHandle) -> Arc<dyn RawDecoder>,
        camera: impl FnOnce(FakeLightHandle) -> Box<dyn CameraSource + Send>,
    ) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gtp-capture-control-{label}-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let (ctx, note) = AppContext::new(&dir.join("settings.json"));
        assert!(note.is_none());
        ctx.create_library(&dir.join("lib"), "Capture").unwrap();
        let roll = roll_create(
            &ctx.library().unwrap(),
            RollInput {
                name: "Camera".into(),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
        // No settle wait: the tests do not sleep.
        ctx.settings()
            .update_capture(&CaptureProfilePatch {
                settle_ms: Some(0),
                ..CaptureProfilePatch::default()
            })
            .unwrap();
        let fake = FakeLight::new();
        let light = fake.handle();
        let slot = Arc::new(CameraSlot::new(fake_camera_info(), camera(light.clone())));
        let hardware = Hardware::from_vars(Some("1"), Some(OsString::from("none"))).with_capture(
            SharedLight::new(Box::new(fake), fake_ports),
            Some(slot),
            decoder(light.clone()),
        );
        let (tx, outcomes) = mpsc::channel();
        Self {
            dir,
            ctx: Some(ctx),
            roll,
            hardware,
            light,
            jobs: JobManager::new(Arc::new(Collect(Mutex::new(tx)))),
            outcomes,
            events: Arc::new(Recorder::default()),
        }
    }

    fn ctx(&self) -> &AppContext {
        self.ctx.as_ref().unwrap()
    }

    fn start(&self) -> Result<JobId, AppError> {
        self.start_with([44, 22, 2])
    }

    fn start_with(&self, levels: [u32; 3]) -> Result<JobId, AppError> {
        start_triplet(
            self.ctx(),
            &self.jobs,
            &self.hardware,
            Arc::new(NoLibraryEvents),
            self.events.clone(),
            self.roll,
            levels,
        )
    }

    fn calibrate(&self, levels: [u32; 3], region: Option<Region>) -> Result<JobId, AppError> {
        start_calibration(
            self.ctx(),
            &self.jobs,
            &self.hardware,
            self.events.clone(),
            self.roll,
            levels,
            region,
        )
    }

    fn outcome(&self) -> Outcome {
        self.outcomes.recv_timeout(WAIT).unwrap()
    }

    fn frames(&self) -> usize {
        self.ctx()
            .library()
            .unwrap()
            .read(|c| c.list_frames(self.roll))
            .unwrap()
            .len()
    }

    fn test(&self, channel: TripletChannel) -> Result<TestShot, AppError> {
        self.test_with(channel, [44, 22, 2])
    }

    fn test_with(&self, channel: TripletChannel, levels: [u32; 3]) -> Result<TestShot, AppError> {
        test_shot(
            self.ctx(),
            &self.hardware,
            self.events.as_ref(),
            channel,
            levels,
        )
    }

    fn set_light(&self, temp_c: Option<f32>, connected: bool) {
        self.light.set_status(LightStatus {
            temp_c,
            vbus_mv: Some(5150),
            connected,
        });
    }

    fn lit_levels(&self) -> Vec<[u8; 3]> {
        self.light
            .commands()
            .into_iter()
            .filter_map(|c| match c {
                LightCommand::SetLevels(l) if l != [0; 3] => Some(l),
                _ => None,
            })
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.ctx = None;
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn assert_busy<T: std::fmt::Debug>(result: Result<T, AppError>) {
    let err = result.unwrap_err();
    assert!(err.message.contains("capture is in progress"), "{err}");
}

fn assert_too_hot<T: std::fmt::Debug>(result: Result<T, AppError>) {
    let err = result.unwrap_err();
    assert!(err.message.contains("too hot"), "{err}");
}

#[cfg(not(all(target_os = "macos", feature = "tether")))]
fn assert_no_camera<T: std::fmt::Debug>(result: Result<T, AppError>) {
    let err = result.unwrap_err();
    assert!(err.message.contains("No camera backend"), "{err}");
}

#[test]
fn the_light_and_camera_commands_work_on_the_fakes() {
    let rig = Rig::new("commands");
    let hw = &rig.hardware;
    assert_eq!(light_list(hw).unwrap()[0].name, "fake");
    assert_eq!(camera_list(hw), [fake_camera_info()]);
    let info = light_connect(hw, Some(" COM5 ")).unwrap();
    assert_eq!(info.port, "fake");
    assert_eq!(
        rig.light.commands()[..2],
        [
            LightCommand::SetLevels([0; 3]),
            LightCommand::Connect(Some("COM5".into()))
        ]
    );
    light_connect(hw, None).unwrap();
    light_set(hw, [10, 20, 30], 55.0).unwrap();
    assert_eq!(rig.light.levels(), [10, 20, 30]);
    light_off(hw).unwrap();
    assert_eq!(rig.light.levels(), [0; 3]);
    assert!(light_connect(hw, Some("COM\n5")).is_err());
    assert!(light_connect(hw, Some(&"x".repeat(300))).is_err());
}

#[test]
fn a_capture_stores_a_frame_reports_it_like_an_import_and_emits_the_steps_in_order() {
    let rig = Rig::new("capture");
    rig.start().unwrap();
    let Outcome::Done(JobResult::Imported(result)) = rig.outcome() else {
        panic!("the job did not finish with a frame");
    };
    assert_eq!(result.frames.len(), 1);
    assert_eq!(rig.frames(), 1);
    assert_eq!(rig.light.levels(), [0; 3]);
    assert_eq!(rig.lit_levels(), [[44, 0, 0], [0, 22, 0], [0, 0, 2]]);
    let expected: Vec<_> = TripletChannel::ALL
        .into_iter()
        .flat_map(|c| [StepState::Lit, StepState::Fired, StepState::Received].map(|s| (c, s)))
        .collect();
    assert_eq!(rig.events.steps(), expected);
    // The busy flag went with the job.
    assert!(!rig.hardware.busy().is_busy());
    light_set(&rig.hardware, [1, 0, 0], 55.0).unwrap();
}

#[test]
fn a_test_shot_measures_the_lit_channel_stores_nothing_and_leaves_the_light_off() {
    let mut calls = None;
    let rig = Rig::build("test-shot", Arc::new(ClipRed), |light| {
        let camera = fake_camera().linked_to(light);
        let handle: FakeCameraHandle = camera.handle();
        calls = Some(handle);
        Box::new(camera)
    });
    let shot = rig.test(TripletChannel::Red).unwrap();
    assert_eq!(shot.histogram.len(), 256);
    assert_eq!(shot.histogram.iter().sum::<u32>(), 192 * 128);
    assert!(shot.clipped_pct > 90.0, "{}", shot.clipped_pct);
    assert!(shot.histogram[255] > 0);
    assert_eq!(rig.light.levels(), [0; 3]);

    // Green is measured on the green channel, which the decoder did not clip.
    let green = rig.test(TripletChannel::Green).unwrap();
    assert_eq!(green.clipped_pct, 0.0);
    assert_eq!(green.histogram[255], 0);
    assert!(green.histogram.iter().skip(100).sum::<u32>() > 0);

    let steps = rig.events.steps();
    assert_eq!(steps.len(), 6);
    assert_eq!(
        steps[3..],
        [
            (TripletChannel::Green, StepState::Lit),
            (TripletChannel::Green, StepState::Fired),
            (TripletChannel::Green, StepState::Received)
        ]
    );
    // Only the chosen channel was lit, at its own profile level, when the camera fired.
    assert_eq!(rig.lit_levels(), [[44, 0, 0], [0, 22, 0]]);
    let fired: Vec<_> = calls
        .unwrap()
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            CameraCall::Trigger { levels } => levels,
            _ => None,
        })
        .collect();
    assert_eq!(fired, [[44, 0, 0], [0, 22, 0]]);
    assert_eq!(rig.light.levels(), [0; 3]);
    assert_eq!(rig.frames(), 0, "a test shot stores nothing");
    assert!(!rig.hardware.busy().is_busy());
}

#[test]
fn a_test_shot_that_fails_still_leaves_the_light_off_and_releases_the_flag() {
    let rig = Rig::build("test-fail", Arc::new(FakeRawDecoder), |light| {
        Box::new(FakeCamera::new(vec![Shot::Fail(no_picture())]).linked_to(light))
    });
    assert_eq!(rig.test(TripletChannel::Red).unwrap_err(), no_picture());
    assert_eq!(rig.light.levels(), [0; 3]);
    assert!(!rig.hardware.busy().is_busy());
    assert_eq!(rig.frames(), 0);
}

#[test]
fn a_test_shot_refuses_a_light_that_is_hot_or_gone() {
    let rig = Rig::new("test-refuse");
    rig.set_light(Some(56.0), true);
    assert_too_hot(rig.test(TripletChannel::Red));
    rig.set_light(Some(25.0), false);
    assert!(rig.test(TripletChannel::Red).is_err());
    assert!(rig.lit_levels().is_empty());
    assert!(!rig.hardware.busy().is_busy());
}

#[test]
fn a_tripped_light_refuses_a_test_shot_and_a_capture_until_it_is_reconnected() {
    let rig = Rig::new("tripped");
    rig.set_light(Some(60.0), true);
    rig.hardware.watch_light(55.0, rig.events.as_ref());
    rig.set_light(Some(30.0), true);
    assert_too_hot(rig.test(TripletChannel::Red));
    assert_too_hot(light_set(&rig.hardware, [5, 0, 0], 55.0));
    rig.start().unwrap();
    let Outcome::Error(err) = rig.outcome() else {
        panic!("a tripped light must not capture");
    };
    assert!(err.message.contains("too hot"), "{err}");
    assert_eq!(rig.frames(), 0);
    assert!(rig.lit_levels().is_empty());

    light_connect(&rig.hardware, None).unwrap();
    rig.start().unwrap();
    assert!(matches!(rig.outcome(), Outcome::Done(_)));
}

#[test]
fn the_watchdog_trips_a_light_that_is_on_while_idle_and_reports_the_status() {
    let rig = Rig::new("idle");
    light_set(&rig.hardware, [44, 0, 0], 55.0).unwrap();
    rig.hardware.watch_light(55.0, rig.events.as_ref());
    assert_eq!(rig.light.levels(), [44, 0, 0]);
    rig.set_light(Some(56.0), true);
    rig.hardware.watch_light(55.0, rig.events.as_ref());
    assert_eq!(rig.light.levels(), [0; 3]);
    let statuses = rig.events.statuses();
    assert_eq!(statuses.len(), 2);
    assert_eq!(
        statuses[0],
        LightReading {
            temp_c: Some(25.0),
            vbus_mv: Some(5150),
            tripped: false
        }
    );
    assert_eq!(statuses[1].temp_c, Some(56.0));
    assert!(statuses[1].tripped);
}

#[test]
fn the_watchdog_gets_through_a_capture_that_waits_for_the_camera() {
    let mut gate = None;
    let rig = Rig::build("blocked", Arc::new(FakeRawDecoder), |light| {
        let (camera, g) = gated(light);
        gate = Some(g);
        camera
    });
    let gate = gate.unwrap();
    rig.start().unwrap();
    gate.entered.recv_timeout(WAIT).unwrap();
    // The red light is on and the camera is blocked in wait_for_image.
    assert_eq!(rig.light.levels(), [44, 0, 0]);
    rig.set_light(Some(60.0), true);
    rig.hardware.watch_light(55.0, rig.events.as_ref());
    assert_eq!(rig.light.levels(), [0; 3], "switched off during the wait");
    assert!(rig.hardware.light.is_tripped());
    assert!(
        rig.hardware.busy().is_busy(),
        "the capture is still running"
    );

    gate.release.send(()).unwrap();
    let Outcome::Error(err) = rig.outcome() else {
        panic!("the capture must fail after a trip");
    };
    assert!(err.message.contains("too hot"), "{err}");
    assert_eq!(rig.lit_levels(), [[44, 0, 0]], "no further colour was lit");
    assert_eq!(rig.light.levels(), [0; 3]);
    assert_eq!(rig.frames(), 0);
    assert!(!rig.hardware.busy().is_busy());
}

#[test]
fn a_trip_between_two_colours_stops_the_sequence_at_the_next_colour() {
    // The sequence on the shared light, the trip arriving while the camera waits for the red
    // picture and with no step wrapper around the camera: the next set_levels is what refuses.
    let fake = FakeLight::new();
    let handle = fake.handle();
    let light = SharedLight::new(Box::new(fake), fake_ports);
    let tripper = light.clone();
    let status = handle.clone();
    let mut camera = fake_camera().on_wait(move |_| {
        status.set_status(LightStatus {
            temp_c: Some(70.0),
            vbus_mv: Some(5150),
            connected: true,
        });
        tripper.poll(55.0);
    });
    let sequence = CaptureSequence::new(CaptureProfile {
        settle_ms: 0,
        ..CaptureProfile::default()
    });
    let mut sequence_light = light.clone();
    let err = sequence
        .run(&mut sequence_light, &mut camera, &|| false, &mut |_| {})
        .unwrap_err();
    assert!(err.message.contains("too hot"), "{err}");
    let lit: Vec<_> = handle
        .commands()
        .into_iter()
        .filter(|c| matches!(c, LightCommand::SetLevels(l) if *l != [0; 3]))
        .collect();
    assert_eq!(lit, [LightCommand::SetLevels([44, 0, 0])]);
    assert_eq!(handle.levels(), [0; 3]);
}

#[test]
fn while_a_capture_runs_every_other_capture_command_is_refused_and_off_still_works() {
    let mut gate = None;
    let rig = Rig::build("busy", Arc::new(FakeRawDecoder), |light| {
        let (camera, g) = gated(light);
        gate = Some(g);
        camera
    });
    let gate = gate.unwrap();
    rig.start().unwrap();
    gate.entered.recv_timeout(WAIT).unwrap();

    assert_busy(rig.start());
    assert_busy(rig.test(TripletChannel::Green));
    assert_busy(light_set(&rig.hardware, [1, 2, 3], 55.0));
    assert_busy(light_connect(&rig.hardware, None));
    // Off and the watchdog are never refused.
    light_off(&rig.hardware).unwrap();
    assert_eq!(rig.light.levels(), [0; 3]);
    rig.hardware.watch_light(55.0, rig.events.as_ref());
    assert_eq!(rig.events.statuses().len(), 1);

    gate.release.send(()).unwrap();
    // Whatever the job ends with, it releases the flag.
    let _ = rig.outcome();
    assert!(!rig.hardware.busy().is_busy());
    light_set(&rig.hardware, [1, 2, 3], 55.0).unwrap();
}

#[test]
fn the_busy_flag_is_released_after_success_error_and_panic() {
    // Success and error of a test shot and of a capture are covered by the tests above. A job
    // whose camera panics:
    let rig = Rig::build("panic", Arc::new(FakeRawDecoder), |_| Box::new(PanicCamera));
    rig.start().unwrap();
    let Outcome::Error(err) = rig.outcome() else {
        panic!("a panicking job must end in an error");
    };
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(!rig.hardware.busy().is_busy());
    assert_eq!(rig.light.levels(), [0; 3], "off after a panic too");
    assert_eq!(rig.frames(), 0);

    // A guard held by a panicking thread is released by the unwind.
    let flag = BusyFlag::default();
    let held = flag.clone();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _guard = held.acquire().unwrap();
        assert!(held.is_busy());
        panic!("work failed");
    }));
    assert!(result.is_err());
    assert!(!flag.is_busy());
    assert!(flag.acquire().is_ok());
}

#[test]
fn without_fake_hardware_the_light_still_answers() {
    let hardware = Hardware::from_vars(None, None);
    // Nothing is connected: switching off is fine, lighting is an error in words.
    light_off(&hardware).unwrap();
    let err = light_set(&hardware, [1, 0, 0], 55.0).unwrap_err();
    assert!(err.message.contains("not connected"), "{err}");
    assert!(!hardware.busy().is_busy());
}

/// Builds without a camera backend refuse up front. With `tether` on macOS, libgphoto2 is the
/// backend and would drive whatever camera is plugged in, so this is not run there: an absent
/// camera with a backend is covered by
/// `an_absent_or_held_camera_fails_a_test_shot_and_a_capture_in_words_before_any_light`, and the
/// real camera by the ignored `gphoto_hardware` test.
#[cfg(not(all(target_os = "macos", feature = "tether")))]
#[test]
fn without_a_camera_backend_a_test_shot_and_a_capture_are_refused() {
    let hardware = Hardware::from_vars(None, None);
    assert!(camera_list(&hardware).is_empty());
    let rig = Rig::new("no-camera");
    assert_no_camera(test_shot(
        rig.ctx(),
        &hardware,
        rig.events.as_ref(),
        TripletChannel::Red,
        [44, 22, 2],
    ));
    assert_no_camera(start_triplet(
        rig.ctx(),
        &rig.jobs,
        &hardware,
        Arc::new(NoLibraryEvents),
        rig.events.clone(),
        rig.roll,
        [44, 22, 2],
    ));
    assert!(!hardware.busy().is_busy());
}

#[test]
fn a_capture_needs_an_open_library_and_a_roll() {
    let rig = Rig::new("no-roll");
    let err = start_triplet(
        rig.ctx(),
        &rig.jobs,
        &rig.hardware,
        Arc::new(NoLibraryEvents),
        rig.events.clone(),
        RollId::new(),
        [44, 22, 2],
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(
        !rig.hardware.busy().is_busy(),
        "a refused start holds nothing"
    );

    let dir = std::env::temp_dir().join(format!("gtp-capture-nolib-{}", FrameId::new()));
    let (empty, _) = AppContext::new(&dir.join("settings.json"));
    let err = start_triplet(
        &empty,
        &rig.jobs,
        &rig.hardware,
        Arc::new(NoLibraryEvents),
        rig.events.clone(),
        rig.roll,
        [44, 22, 2],
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn light_set_needs_a_connected_light_with_a_current_reading_under_the_limit() {
    let rig = Rig::new("set-checks");
    let hw = &rig.hardware;
    let set = |levels| light_set_waiting(hw, levels, 55.0, Duration::ZERO);
    // Allowed.
    set([10, 20, 30]).unwrap();
    assert_eq!(rig.light.levels(), [10, 20, 30]);
    light_off(hw).unwrap();
    // No reading, too hot, not connected: refused, and nothing is lit.
    for (temp_c, connected, words) in [
        (None, true, "not reporting"),
        (Some(56.0), true, "too hot"),
        (Some(25.0), false, "not connected"),
    ] {
        rig.set_light(temp_c, connected);
        let err = set([1, 2, 3]).unwrap_err();
        assert!(err.message.contains(words), "{err}");
        assert_eq!(rig.light.levels(), [0; 3], "{words}");
    }
    // All zeros are always allowed, whatever the light says.
    set([0; 3]).unwrap();
    assert!(!hw.busy().is_busy());
}

/// A finder that answers from a script: what the list gives, and whether `connect` fails.
#[derive(Default)]
struct ScriptedFinder {
    lists: Mutex<std::collections::VecDeque<Result<Vec<CameraInfo>, AppError>>>,
    connect_error: Mutex<Option<AppError>>,
    connects: std::sync::atomic::AtomicUsize,
}

struct SharedFinder(Arc<ScriptedFinder>);

impl CameraFinder for SharedFinder {
    fn list(&self) -> Result<Vec<CameraInfo>, AppError> {
        self.0
            .lists
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(vec![]))
    }

    fn connect(&self) -> Result<CameraInfo, AppError> {
        self.0.connects.fetch_add(1, Ordering::SeqCst);
        match self.0.connect_error.lock().unwrap().clone() {
            Some(err) => Err(err),
            None => Ok(fake_camera_info()),
        }
    }
}

/// A rig whose camera is found through `finder` (the camera itself is the fake one).
fn found_rig(label: &str, finder: &Arc<ScriptedFinder>) -> Rig {
    let mut rig = Rig::new(label);
    let camera = Box::new(fake_camera().linked_to(rig.light.clone()));
    rig.hardware.camera = Some(Arc::new(CameraSlot::with_finder(
        camera,
        Box::new(SharedFinder(Arc::clone(finder))),
    )));
    rig
}

#[test]
fn a_camera_found_later_is_listed_and_the_last_list_stands_when_the_backend_cannot_answer() {
    let finder = Arc::new(ScriptedFinder::default());
    let rig = found_rig("finder-list", &finder);
    // Not plugged in at start: an empty list, not an error.
    assert!(camera_list(&rig.hardware).is_empty());
    finder
        .lists
        .lock()
        .unwrap()
        .extend([Ok(vec![fake_camera_info()]), Err(busy())]);
    assert_eq!(camera_list(&rig.hardware), [fake_camera_info()]);
    // A picture is being taken: the backend refuses, the last list is kept.
    assert_eq!(camera_list(&rig.hardware), [fake_camera_info()]);
}

#[test]
fn an_absent_or_held_camera_fails_a_test_shot_and_a_capture_in_words_before_any_light() {
    let finder = Arc::new(ScriptedFinder::default());
    let rig = found_rig("finder-absent", &finder);
    let words = AppError::new(ErrorCode::NotFound, "No camera was found. Connect it.");
    *finder.connect_error.lock().unwrap() = Some(words.clone());

    assert_eq!(rig.test(TripletChannel::Red).unwrap_err(), words);
    rig.start().unwrap();
    match rig.outcome() {
        Outcome::Error(err) => assert_eq!(err, words),
        Outcome::Done(_) => panic!("a capture without a camera must fail"),
    }
    assert!(rig.lit_levels().is_empty(), "no light without a camera");
    assert_eq!(rig.frames(), 0);
    assert!(!rig.hardware.busy().is_busy());

    // Plugged in now: the same commands find it and work, with no restart.
    *finder.connect_error.lock().unwrap() = None;
    rig.test(TripletChannel::Red).unwrap();
    rig.start().unwrap();
    assert!(matches!(rig.outcome(), Outcome::Done(_)));
    assert_eq!(rig.frames(), 1);
    assert!(finder.connects.load(Ordering::SeqCst) >= 4);
}

// --- levels (H-15) ---------------------------------------------------------------------------

mod levels {
    use gtp_core::{ExposureRatio, ImportWarningKind};
    use gtp_ingest::testing::tiff_with_exposure;

    use super::*;
    use crate::services::capture_levels::{
        frame_capture_info, levels_for_roll, save_levels, LevelsScope, LevelsSource, UsedSource,
    };
    use crate::services::roll_frame::roll_update;

    fn exposure(iso: u32) -> ExposureInfo {
        ExposureInfo {
            exposure_time: Some(ExposureRatio { num: 1, den: 60 }),
            iso: Some(iso),
            f_number: Some(ExposureRatio { num: 56, den: 10 }),
        }
    }

    /// A rig whose camera takes every picture at 1/60 s, ISO 200, f/5.6 (as the EXIF says).
    fn rig(label: &str) -> Rig {
        Rig::build(label, Arc::new(FakeRawDecoder), |light| {
            Box::new(fake_camera_with_exif(light))
        })
    }

    fn fake_camera_with_exif(light: FakeLightHandle) -> FakeCamera {
        FakeCamera::new(Vec::new())
            .repeating(CapturedFile {
                name: "FAKE_0001.NEF".to_owned(),
                bytes: tiff_with_exposure((1, 60), 200, (56, 10)),
            })
            .linked_to(light)
    }

    fn set_stock(rig: &Rig, stock: &str) {
        let library = rig.ctx().library().unwrap();
        roll_update(
            &library,
            rig.roll,
            RollInput {
                name: "Camera".into(),
                film_stock: Some(stock.into()),
                ..Default::default()
            },
        )
        .unwrap();
    }

    fn capture(rig: &Rig, levels: [u32; 3]) -> gtp_core::ImportResult {
        rig.start_with(levels).unwrap();
        match rig.outcome() {
            Outcome::Done(JobResult::Imported(result)) => result,
            _ => panic!("the capture did not finish with a frame"),
        }
    }

    fn differs(result: &gtp_core::ImportResult) -> bool {
        result
            .warnings
            .iter()
            .any(|w| w.kind == ImportWarningKind::ExposureDiffersFromLevels)
    }

    #[test]
    fn a_test_shot_lights_exactly_the_given_levels_and_returns_the_exposure() {
        let rig = rig("levels-test");
        let shot = rig.test_with(TripletChannel::Green, [10, 77, 5]).unwrap();
        assert_eq!(shot.exposure, exposure(200));
        assert_eq!(rig.lit_levels(), [[0, 77, 0]]);
        // The profile's levels are not read, and not changed.
        assert_eq!(rig.ctx().settings().capture_profile().levels, [44, 22, 2]);

        let err = rig.test_with(TripletChannel::Red, [0, 256, 0]).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert_eq!(rig.lit_levels().len(), 1, "a refused level lights nothing");
        assert!(!rig.hardware.busy().is_busy());
    }

    #[test]
    fn a_test_shot_of_a_picture_without_exif_has_an_empty_exposure() {
        let rig = Rig::new("levels-no-exif");
        let shot = rig.test(TripletChannel::Red).unwrap();
        assert!(shot.exposure.is_empty());
    }

    #[test]
    fn a_capture_lights_the_given_levels_and_records_them_with_the_frame() {
        let rig = rig("levels-capture");
        let result = capture(&rig, [50, 25, 3]);
        assert_eq!(rig.lit_levels(), [[50, 0, 0], [0, 25, 0], [0, 0, 3]]);
        assert_eq!(rig.ctx().settings().capture_profile().levels, [44, 22, 2]);
        let info = frame_capture_info(rig.ctx(), result.frames[0])
            .unwrap()
            .expect("a captured frame has capture info");
        assert_eq!(info.levels, [50, 25, 3]);
        assert_eq!(info.source, UsedSource::Manual, "no saved levels were used");
        assert_eq!(info.stock, None);
        assert_eq!(info.exposure, [exposure(200); 3]);
        assert!(result.warnings.is_empty());

        let err = rig.start_with([1, 2, 300]).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(!rig.hardware.busy().is_busy());
    }

    #[test]
    fn levels_that_equal_the_saved_ones_are_recorded_as_the_stock_or_the_default() {
        let rig = rig("levels-source");
        // No stock on the roll: the default (the profile levels) is the source.
        let result = capture(&rig, [44, 22, 2]);
        let info = frame_capture_info(rig.ctx(), result.frames[0])
            .unwrap()
            .unwrap();
        assert_eq!((info.source, info.stock), (UsedSource::Default, None));

        set_stock(&rig, "  portra 400 ");
        // A stock with no saved levels still uses the default.
        assert_eq!(
            levels_for_roll(rig.ctx(), rig.roll).unwrap().source,
            LevelsSource::Default
        );
        let saved = save_levels(
            rig.ctx(),
            &LevelsScope::Stock("Portra 400".into()),
            [60, 30, 4],
            Some(exposure(200)),
        )
        .unwrap();
        assert_eq!(saved.source, LevelsSource::Stock);
        let resolved = levels_for_roll(rig.ctx(), rig.roll).unwrap();
        assert_eq!(
            resolved, saved,
            "matched without case or surrounding spaces"
        );

        let result = capture(&rig, [60, 30, 4]);
        assert_eq!(
            rig.lit_levels().split_off(3),
            [[60, 0, 0], [0, 30, 0], [0, 0, 4]]
        );
        let info = frame_capture_info(rig.ctx(), result.frames[0])
            .unwrap()
            .unwrap();
        assert_eq!(info.source, UsedSource::Stock);
        assert_eq!(info.stock.as_deref(), Some("Portra 400"));
        assert!(result.warnings.is_empty(), "the same camera settings");

        // A slider moved: manual, whatever the stock of the roll.
        let result = capture(&rig, [61, 30, 4]);
        let info = frame_capture_info(rig.ctx(), result.frames[0])
            .unwrap()
            .unwrap();
        assert_eq!((info.source, info.stock), (UsedSource::Manual, None));
    }

    #[test]
    fn a_capture_warns_when_the_camera_settings_differ_from_the_saved_levels() {
        let rig = rig("levels-warn");
        set_stock(&rig, "Portra 400");
        // Levels saved at ISO 400; the camera takes ISO 200.
        save_levels(
            rig.ctx(),
            &LevelsScope::Stock("Portra 400".into()),
            [60, 30, 4],
            Some(exposure(400)),
        )
        .unwrap();
        assert!(differs(&capture(&rig, [60, 30, 4])));
        // Manual levels compare with nothing.
        assert!(!differs(&capture(&rig, [61, 30, 4])));

        // The same for the default, on a roll whose stock has no saved levels.
        set_stock(&rig, "Ilford HP5");
        assert!(!differs(&capture(&rig, [44, 22, 2])), "no saved settings");
        let default = |exposure: Option<ExposureInfo>| {
            save_levels(rig.ctx(), &LevelsScope::Default, [44, 22, 2], exposure).unwrap();
        };
        default(Some(exposure(100)));
        assert!(differs(&capture(&rig, [44, 22, 2])));
        // Saved with the settings the camera uses: no warning.
        default(Some(exposure(200)));
        assert!(!differs(&capture(&rig, [44, 22, 2])));
        // Saved with no settings (a test picture without EXIF): nothing to compare.
        default(None);
        assert!(!differs(&capture(&rig, [44, 22, 2])));
    }

    #[test]
    fn saving_levels_validates_and_the_default_changes_the_profile() {
        let rig = Rig::new("levels-save");
        let err = save_levels(rig.ctx(), &LevelsScope::Default, [300, 0, 0], None).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let err =
            save_levels(rig.ctx(), &LevelsScope::Stock("  ".into()), [1, 1, 1], None).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert_eq!(rig.ctx().settings().capture_profile().levels, [44, 22, 2]);

        let saved = save_levels(
            rig.ctx(),
            &LevelsScope::Default,
            [70, 35, 5],
            Some(ExposureInfo::default()),
        )
        .unwrap();
        assert_eq!(saved.levels, [70, 35, 5]);
        assert_eq!(saved.exposure, None, "an empty exposure is not kept");
        assert_eq!(rig.ctx().settings().capture_profile().levels, [70, 35, 5]);
        assert_eq!(
            levels_for_roll(rig.ctx(), rig.roll).unwrap().levels,
            [70, 35, 5]
        );
        let err = levels_for_roll(rig.ctx(), RollId::new()).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }
}

// --- the camera's exposure settings (H-16) -------------------------------------------------------

/// A rig whose camera slot gives access to the settings of a fake camera, and that camera's handle.
fn settings_rig(label: &str) -> (Rig, FakeCameraHandle) {
    let mut rig = Rig::new(label);
    let camera = fake_camera().linked_to(rig.light.clone());
    let handle = camera.handle();
    rig.hardware.camera = Some(Arc::new(
        CameraSlot::new(fake_camera_info(), Box::new(camera))
            .with_settings(Box::new(handle.clone())),
    ));
    (rig, handle)
}

/// Only where there is no backend: with `tether` on macOS, libgphoto2 is the backend and this would
/// change the ISO of whatever camera is plugged in. The fake camera covers the commands there.
#[cfg(not(all(target_os = "macos", feature = "tether")))]
#[test]
fn without_a_camera_backend_the_settings_commands_say_so() {
    let real = Hardware::from_vars(None, None);
    let err = camera_settings(&real).unwrap_err();
    assert!(err.message.contains("No camera backend"), "{err}");
    let err = camera_set_setting(&real, CameraSettingKind::Iso, "200").unwrap_err();
    assert!(err.message.contains("No camera backend"), "{err}");
}

#[test]
fn the_settings_are_read_and_a_change_is_trimmed_set_and_answered_with_the_new_settings() {
    let (rig, handle) = settings_rig("settings-set");
    let first = camera_settings(&rig.hardware).unwrap();
    assert_eq!(first.len(), 3);
    assert_eq!(first[0].kind, CameraSettingKind::Iso);
    assert_eq!(first[0].value.as_deref(), Some("100"));
    assert_eq!(first[0].choices, ["100", "200", "400", "800"]);

    let after = camera_set_setting(&rig.hardware, CameraSettingKind::Iso, "  400 ").unwrap();
    assert_eq!(after[0].value.as_deref(), Some("400"));
    assert_eq!(camera_settings(&rig.hardware).unwrap(), after);
    assert_eq!(
        handle.changes(),
        [(CameraSettingKind::Iso, "400".to_owned())]
    );
    assert!(!rig.hardware.busy().is_busy(), "the flag is released");
}

#[test]
fn a_value_that_is_too_long_odd_or_not_offered_never_reaches_the_camera() {
    let (rig, handle) = settings_rig("settings-bad");
    for bad in ["", "   ", "1\u{0}00", &"9".repeat(200), "300"] {
        let err = camera_set_setting(&rig.hardware, CameraSettingKind::Iso, bad).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput, "{bad:?}");
        assert!(err.message.contains("not one the camera offers"), "{err}");
    }
    assert!(handle.changes().is_empty());
}

#[test]
fn readonly_and_absent_settings_are_reported_and_refused() {
    let (rig, handle) = settings_rig("settings-readonly");
    handle.make_setting_readonly(CameraSettingKind::ShutterSpeed);
    handle.make_setting_absent(CameraSettingKind::Aperture);
    let got = camera_settings(&rig.hardware).unwrap();
    assert!(!got[1].writable && got[1].value.is_some());
    assert_eq!(got[2], CameraSetting::absent(CameraSettingKind::Aperture));
    let err =
        camera_set_setting(&rig.hardware, CameraSettingKind::ShutterSpeed, "1/60").unwrap_err();
    assert!(err.message.contains("M mode"), "{err}");
    assert!(camera_set_setting(&rig.hardware, CameraSettingKind::Aperture, "8").is_err());
}

#[test]
fn the_settings_are_refused_while_a_capture_or_test_shot_holds_the_camera() {
    let (rig, handle) = settings_rig("settings-busy");
    let _held = rig.hardware.busy().acquire().unwrap();
    let read = camera_settings(&rig.hardware).unwrap_err();
    let set = camera_set_setting(&rig.hardware, CameraSettingKind::Iso, "200").unwrap_err();
    for err in [read, set] {
        assert!(err.message.contains("capture is in progress"), "{err}");
    }
    assert!(handle.changes().is_empty());
}

#[test]
fn a_camera_without_settings_access_says_so_and_errors_come_through_in_words() {
    let rig = Rig::new("settings-none");
    let err = camera_settings(&rig.hardware).unwrap_err();
    assert!(err.message.contains("exposure settings"), "{err}");

    let (rig, handle) = settings_rig("settings-fail");
    let words = AppError::new(ErrorCode::Io, "The camera is busy. Wait a moment.");
    handle.fail_settings_with(Some(words.clone()));
    assert_eq!(camera_settings(&rig.hardware).unwrap_err(), words);
    assert!(!rig.hardware.busy().is_busy());
}

#[test]
fn a_camera_found_through_a_finder_is_connected_before_its_settings_are_used() {
    let finder = Arc::new(ScriptedFinder::default());
    let mut rig = Rig::new("settings-found");
    let camera = fake_camera();
    let handle = camera.handle();
    rig.hardware.camera = Some(Arc::new(
        CameraSlot::with_finder(
            Box::new(camera),
            Box::new(SharedFinder(Arc::clone(&finder))),
        )
        .with_settings(Box::new(handle)),
    ));
    let words = AppError::new(ErrorCode::NotFound, "No camera was found. Connect it.");
    *finder.connect_error.lock().unwrap() = Some(words.clone());
    assert_eq!(camera_settings(&rig.hardware).unwrap_err(), words);
    *finder.connect_error.lock().unwrap() = None;
    assert_eq!(camera_settings(&rig.hardware).unwrap().len(), 3);
    assert_eq!(finder.connects.load(Ordering::SeqCst), 2);
}

fn calibrated(rig: &Rig) -> gtp_core::CalibrationResult {
    match rig.outcome() {
        Outcome::Done(JobResult::Calibrated(result)) => result,
        Outcome::Done(_) => panic!("the job did not end with a calibration"),
        Outcome::Error(err) => panic!("the calibration failed: {err}"),
    }
}

#[test]
fn a_calibration_converges_on_the_fakes_stores_nothing_and_leaves_the_light_off() {
    let rig = Rig::levelled("calibrate");
    let settings_before = std::fs::read_to_string(rig.dir.join("settings.json")).unwrap();
    let profile_before = rig.ctx().settings().capture_profile();
    rig.calibrate([44, 22, 2], None).unwrap();
    let result = calibrated(&rig);

    // Red and green settle inside the range. Blue's default level is 2, where one step of the
    // light is a fifth of the picture's brightness: it ends just below clipping, with a warning.
    for i in 0..2 {
        assert!(
            (0.90..=0.97).contains(&result.reached[i]),
            "{i}: {result:?}"
        );
    }
    assert!(
        result.reached[2] > 0.7 && result.reached[2] < 0.99,
        "{result:?}"
    );
    assert!(result.levels[0] > 44 && result.levels[1] > 22, "{result:?}");
    assert_eq!(
        result.warnings,
        [gtp_core::CalibrationWarning::CoarseLevel {
            channel: TripletChannel::Blue,
            level: result.levels[2]
        }]
    );
    assert!(result.shots.iter().all(|n| (1..=6).contains(n)));

    // One colour lit at a time, in order; light off afterwards; the flag released.
    let lit = rig.lit_levels();
    assert_eq!(
        lit.len(),
        result.shots.iter().map(|n| usize::from(*n)).sum::<usize>()
    );
    assert!(lit
        .iter()
        .all(|l| l.iter().filter(|v| **v > 0).count() == 1));
    assert_eq!(rig.light.levels(), [0; 3]);
    assert!(!rig.hardware.busy().is_busy());
    assert_eq!(rig.events.steps().len(), 3 * lit.len());

    // Nothing was saved or stored.
    assert_eq!(rig.frames(), 0);
    assert_eq!(rig.ctx().settings().capture_profile(), profile_before);
    assert_eq!(
        std::fs::read_to_string(rig.dir.join("settings.json")).unwrap(),
        settings_before
    );
}

#[test]
fn a_calibration_starts_from_any_level_and_measures_only_the_chosen_area() {
    let rig = Rig::levelled("calibrate-region");
    // Halving from 255 takes more than six shots to reach blue's few levels, so it starts near them.
    rig.calibrate([255, 255, 8], None).unwrap();
    let whole = calibrated(&rig);
    for i in 0..2 {
        assert!((0.90..=0.97).contains(&whole.reached[i]), "{whole:?}");
    }
    // The fake's picture is darker at the top left, so that corner needs a brighter light.
    let corner = Region {
        x: 0.0,
        y: 0.0,
        w: 0.25,
        h: 0.25,
    };
    rig.calibrate([44, 22, 2], Some(corner)).unwrap();
    let part = calibrated(&rig);
    assert!(
        part.levels[0] > whole.levels[0] + 20,
        "{part:?} vs {whole:?}"
    );
    assert!((0.90..=0.97).contains(&part.reached[0]), "{part:?}");
}

#[test]
fn a_calibration_that_cannot_succeed_says_why_and_leaves_the_light_off() {
    // Red is clipped whatever the level.
    let rig = Rig::build("calibrate-clipped", Arc::new(ClipRed), |light| {
        Box::new(fake_camera().linked_to(light))
    });
    rig.calibrate([44, 22, 2], None).unwrap();
    let Outcome::Error(err) = rig.outcome() else {
        panic!("a clipped red must fail the calibration");
    };
    assert_eq!(
        err.message,
        "Red is clipped even at level 1: the exposure is too long. Use a shorter shutter speed and calibrate again."
    );
    assert_eq!(rig.light.levels(), [0; 3]);
    assert!(!rig.hardware.busy().is_busy());
    assert_eq!(rig.frames(), 0);
}

#[test]
fn a_calibration_is_refused_while_busy_for_a_bad_area_or_levels_or_roll() {
    let rig = Rig::levelled("calibrate-refused");
    {
        let _busy = rig.hardware.busy().acquire().unwrap();
        assert_busy(rig.calibrate([44, 22, 2], None));
    }
    let nan = Region {
        x: f32::NAN,
        y: 0.0,
        w: 0.5,
        h: 0.5,
    };
    let outside = Region {
        x: 0.6,
        y: 0.0,
        w: 0.5,
        h: 0.5,
    };
    let empty = Region {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.5,
    };
    for region in [nan, outside, empty] {
        let err = rig.calibrate([44, 22, 2], Some(region)).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput, "{region:?}");
    }
    assert_eq!(
        rig.calibrate([44, 22, 256], None).unwrap_err().code,
        ErrorCode::InvalidInput
    );
    let err = start_calibration(
        rig.ctx(),
        &rig.jobs,
        &rig.hardware,
        rig.events.clone(),
        RollId::new(),
        [44, 22, 2],
        None,
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(!rig.hardware.busy().is_busy(), "a refusal holds nothing");
    assert!(rig.lit_levels().is_empty());
}

// Not with `tether` on macOS: libgphoto2 is the backend there, and this would drive whatever
// camera is plugged in.
#[cfg(not(all(target_os = "macos", feature = "tether")))]
#[test]
fn a_calibration_without_a_camera_backend_says_so() {
    let rig = Rig::new("calibrate-no-camera");
    let real = Hardware::from_vars(None, None);
    assert_no_camera(start_calibration(
        rig.ctx(),
        &rig.jobs,
        &real,
        rig.events.clone(),
        rig.roll,
        [44, 22, 2],
        None,
    ));
}
