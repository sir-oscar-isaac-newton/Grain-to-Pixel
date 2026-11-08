//! The worker against a scripted session (built and run on every platform), and the conformance
//! scenarios that `FakeCamera` and the worker must both pass. Timeouts here are tens of
//! milliseconds; nothing sleeps for real except while waiting for a thread to reach a state.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use gtp_core::{AppError, ErrorCode};

use super::paths::bundled_library_dirs;
use super::worker::{
    GphotoCamera, Session, SessionError, SessionErrorKind, Timeouts, CLAIM_MARKER,
};
use crate::camera::{CameraFinder, CameraInfo, CameraSource, CapturedFile};
use crate::fakes::{no_picture, FakeCamera, FakeLight, LightCommand, Shot};
use crate::preview::CameraPreview;
use crate::profile::CaptureProfile;
use crate::sequence::{CaptureSequence, Pause};
use crate::settings::{CameraSetting, CameraSettingKind, CameraSettings};

const SHORT: Duration = Duration::from_millis(30);

fn file(n: u8) -> CapturedFile {
    CapturedFile {
        name: format!("DSC_{n:04}.NEF"),
        bytes: vec![n; 4],
    }
}

fn info() -> CameraInfo {
    CameraInfo {
        id: "gphoto:Nikon DSC D600".to_owned(),
        name: "Nikon DSC D600".to_owned(),
    }
}

fn other(message: &str) -> SessionError {
    SessionError::new(SessionErrorKind::Other, message)
}

/// A door a scripted capture can be held at until the test opens it.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn release(&self) {
        *self.open.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.changed.notify_all();
    }

    fn wait(&self) {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        while !*open {
            open = self
                .changed
                .wait(open)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// What one scripted `capture` does.
enum Step {
    File(CapturedFile),
    Fail(SessionError),
    /// Blocks until the gate opens, then returns the file (a hung camera that comes back late).
    Hold(Arc<Gate>, CapturedFile),
}

#[derive(Default)]
struct Script {
    log: Mutex<Vec<String>>,
    opens: Mutex<VecDeque<Result<CameraInfo, SessionError>>>,
    lists: Mutex<VecDeque<Result<Vec<CameraInfo>, SessionError>>>,
    captures: Mutex<VecDeque<Step>>,
    finished: AtomicUsize,
    /// What `settings` reads; `set_setting` changes it.
    settings: Mutex<Vec<CameraSetting>>,
    /// Makes `settings` and `set_setting` fail once.
    settings_error: Mutex<Option<SessionError>>,
    /// Makes `settings` wait at this gate (a camera that stops answering).
    settings_gate: Mutex<Option<Arc<Gate>>>,
    /// Makes `preview_frame` fail once.
    preview_error: Mutex<Option<SessionError>>,
    /// Makes `leave_live_view` fail once.
    leave_error: Mutex<Option<SessionError>>,
}

impl Script {
    fn record(&self, entry: &str) {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry.to_owned());
    }

    fn log(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn count(&self, entry: &str) -> usize {
        self.log().iter().filter(|e| *e == entry).count()
    }

    fn current_settings(&self) -> Vec<CameraSetting> {
        self.settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn with_settings(self, settings: Vec<CameraSetting>) -> Self {
        *self.settings.lock().unwrap_or_else(PoisonError::into_inner) = settings;
        self
    }

    fn push_capture(&self, step: Step) {
        self.captures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(step);
    }
}

struct Scripted(Arc<Script>);

impl Session for Scripted {
    fn list(&mut self) -> Result<Vec<CameraInfo>, SessionError> {
        self.0.record("list");
        self.0
            .lists
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| Ok(vec![info()]))
    }

    fn open(&mut self) -> Result<CameraInfo, SessionError> {
        self.0.record("open");
        self.0
            .opens
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| Ok(info()))
    }

    fn capture(&mut self) -> Result<CapturedFile, SessionError> {
        self.0.record("capture");
        let step = self
            .0
            .captures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
        let result = match step {
            Some(Step::File(file)) => Ok(file),
            Some(Step::Fail(err)) => Err(err),
            Some(Step::Hold(gate, file)) => {
                gate.wait();
                Ok(file)
            }
            None => Err(other("nothing scripted")),
        };
        self.0.finished.fetch_add(1, Ordering::SeqCst);
        result
    }

    fn settings(&mut self) -> Result<Vec<CameraSetting>, SessionError> {
        self.0.record("settings");
        let gate = self
            .0
            .settings_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(gate) = gate {
            gate.wait();
        }
        if let Some(err) = self
            .0
            .settings_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            return Err(err);
        }
        Ok(self.0.current_settings())
    }

    fn set_setting(&mut self, kind: CameraSettingKind, value: &str) -> Result<(), SessionError> {
        self.0.record(&format!("set {kind:?} {value}"));
        let mut settings = self
            .0
            .settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(s) = settings.iter_mut().find(|s| s.kind == kind) {
            s.value = Some(value.to_owned());
        }
        Ok(())
    }

    fn preview_frame(&mut self) -> Result<Vec<u8>, SessionError> {
        self.0.record("preview");
        if let Some(err) = self
            .0
            .preview_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            return Err(err);
        }
        Ok(vec![0xFF, 0xD8, 0xFF, 0xD9])
    }

    fn leave_live_view(&mut self) -> Result<(), SessionError> {
        self.0.record("leave");
        match self
            .0
            .leave_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    fn close(&mut self) {
        self.0.record("close");
    }
}

fn camera_with(script: &Arc<Script>, timeouts: Timeouts) -> GphotoCamera {
    let script = Arc::clone(script);
    GphotoCamera::with_timeouts(
        move || Ok(Box::new(Scripted(Arc::clone(&script))) as Box<dyn Session>),
        timeouts,
    )
    .unwrap()
}

fn camera(script: &Arc<Script>) -> GphotoCamera {
    camera_with(
        script,
        Timeouts {
            open: Duration::from_secs(5),
            list: Duration::from_secs(5),
            settings: Duration::from_secs(5),
            preview: Duration::from_secs(5),
        },
    )
}

/// Polls until `done`, for at most 10 s (it is normally true at once).
fn eventually(mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "the worker never got there");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn shoot(camera: &mut dyn CameraSource, timeout: Duration) -> Result<CapturedFile, AppError> {
    camera.arm()?;
    camera.trigger()?;
    camera.wait_for_image(timeout)
}

// --- the worker --------------------------------------------------------------------------------

#[test]
fn pictures_come_in_the_order_taken_and_the_camera_is_opened_once() {
    let script = Arc::new(Script::default());
    for n in 1..=3 {
        script.push_capture(Step::File(file(n)));
    }
    let mut camera = camera(&script);
    assert!(
        !camera.is_connected(),
        "nothing is opened before it is needed"
    );
    camera.connect().unwrap();
    assert!(camera.is_connected());
    for n in 1..=3 {
        assert_eq!(shoot(&mut camera, Duration::from_secs(5)).unwrap(), file(n));
    }
    assert_eq!(script.count("open"), 1);
    assert_eq!(script.count("capture"), 3);
}

#[test]
fn trigger_returns_before_the_picture_exists() {
    let script = Arc::new(Script::default());
    let gate = Arc::new(Gate::default());
    script.push_capture(Step::Hold(Arc::clone(&gate), file(1)));
    let mut camera = camera(&script);
    camera.arm().unwrap();
    camera.trigger().unwrap();
    // The capture is held at the gate, and trigger has already returned.
    assert!(camera.is_connected());
    gate.release();
    assert_eq!(
        camera.wait_for_image(Duration::from_secs(5)).unwrap(),
        file(1)
    );
}

#[test]
fn a_request_that_times_out_makes_the_camera_unusable_with_words() {
    let script = Arc::new(Script::default());
    let gate = Arc::new(Gate::default());
    script.push_capture(Step::Hold(Arc::clone(&gate), file(1)));
    let mut camera = camera(&script);
    camera.arm().unwrap();
    camera.trigger().unwrap();

    let err = camera.wait_for_image(SHORT).unwrap_err();
    assert_eq!(err.code, ErrorCode::Io);
    assert!(
        err.message.contains("did not answer in time"),
        "{}",
        err.message
    );
    assert!(err.message.contains("restart the app"));
    assert!(camera.is_unusable());
    assert!(!camera.is_connected());

    // Every later call fails at once, without waiting for the stuck thread.
    for result in [camera.arm(), camera.trigger(), camera.connect().map(|_| ())] {
        let err = result.unwrap_err();
        assert!(err.message.contains("stopped answering"), "{}", err.message);
    }
    gate.release();
}

#[test]
fn a_late_result_is_discarded_and_never_becomes_the_next_picture() {
    let script = Arc::new(Script::default());
    let gate = Arc::new(Gate::default());
    script.push_capture(Step::Hold(Arc::clone(&gate), file(1)));
    script.push_capture(Step::File(file(2)));
    let mut camera = camera(&script);
    camera.arm().unwrap();
    camera.trigger().unwrap();
    assert!(camera.wait_for_image(SHORT).is_err());

    // The stuck call returns late: its picture goes nowhere.
    gate.release();
    eventually(|| script.finished.load(Ordering::SeqCst) == 1);
    assert!(camera.wait_for_image(SHORT).is_err(), "nothing is waiting");
    assert!(
        camera.arm().is_err(),
        "still unusable after the late return"
    );

    // After a reconnect the next picture is the next one in the script, not the late one.
    camera.reconnect().unwrap();
    assert!(!camera.is_unusable());
    assert_eq!(shoot(&mut camera, Duration::from_secs(5)).unwrap(), file(2));
}

#[test]
fn a_reconnect_starts_a_fresh_worker_while_the_old_one_is_still_stuck() {
    let script = Arc::new(Script::default());
    let gate = Arc::new(Gate::default());
    script.push_capture(Step::Hold(Arc::clone(&gate), file(1)));
    script.push_capture(Step::File(file(2)));
    let mut camera = camera(&script);
    camera.arm().unwrap();
    camera.trigger().unwrap();
    assert!(camera.wait_for_image(SHORT).is_err());

    camera.reconnect().unwrap();
    // The old worker is still inside capture 1, yet the new one serves capture 2.
    assert_eq!(shoot(&mut camera, Duration::from_secs(5)).unwrap(), file(2));
    gate.release();
    eventually(|| script.finished.load(Ordering::SeqCst) == 2);
    // The old worker saw it was abandoned and let go of its camera.
    eventually(|| script.count("close") >= 1);
}

#[test]
fn refreshing_the_list_is_the_way_back_from_an_unusable_camera() {
    let script = Arc::new(Script::default());
    let gate = Arc::new(Gate::default());
    script.push_capture(Step::Hold(Arc::clone(&gate), file(1)));
    let mut camera = camera(&script);
    let finder = camera.finder();
    camera.arm().unwrap();
    camera.trigger().unwrap();
    assert!(camera.wait_for_image(SHORT).is_err());
    assert!(camera.is_unusable());

    assert_eq!(finder.list().unwrap(), [info()]);
    assert!(!camera.is_unusable());
    gate.release();
}

#[test]
fn dropping_the_camera_while_it_is_stuck_does_not_wait_for_the_thread() {
    let script = Arc::new(Script::default());
    let gate = Arc::new(Gate::default());
    script.push_capture(Step::Hold(Arc::clone(&gate), file(1)));
    let mut camera = camera(&script);
    camera.arm().unwrap();
    camera.trigger().unwrap();
    eventually(|| script.count("capture") == 1);

    let (done, finished) = channel();
    std::thread::spawn(move || {
        drop(camera);
        // The receiver is only gone if the test already failed.
        let _ = done.send(());
    });
    finished
        .recv_timeout(Duration::from_secs(10))
        .expect("dropping the camera hung on the stuck thread");

    // Once the call returns, the abandoned worker ends and lets go of the camera.
    gate.release();
    eventually(|| script.count("close") == 1);
}

#[test]
fn the_state_is_answered_without_touching_the_blocked_worker() {
    let script = Arc::new(Script::default());
    let gate = Arc::new(Gate::default());
    script.push_capture(Step::Hold(Arc::clone(&gate), file(1)));
    let mut camera = camera(&script);
    camera.arm().unwrap();
    camera.trigger().unwrap();
    eventually(|| script.count("capture") == 1);
    // The worker is blocked inside capture; these return at once.
    assert!(camera.is_connected());
    assert!(!camera.is_unusable());
    gate.release();
    assert_eq!(
        camera.wait_for_image(Duration::from_secs(5)).unwrap(),
        file(1)
    );
}

#[test]
fn listing_and_connecting_are_refused_while_a_picture_is_being_taken() {
    let script = Arc::new(Script::default());
    let gate = Arc::new(Gate::default());
    script.push_capture(Step::Hold(Arc::clone(&gate), file(1)));
    let mut camera = camera(&script);
    let finder = camera.finder();
    camera.arm().unwrap();
    camera.trigger().unwrap();
    for err in [finder.list().unwrap_err(), finder.connect().unwrap_err()] {
        assert!(err.message.contains("being taken"), "{}", err.message);
    }
    assert!(!camera.is_unusable(), "busy is not a failure of the camera");
    gate.release();
    camera.wait_for_image(Duration::from_secs(5)).unwrap();
    assert_eq!(finder.list().unwrap(), [info()]);
}

#[test]
fn a_session_that_cannot_be_built_gives_an_error_in_words() {
    let camera = GphotoCamera::with_session(|| Err(other("no context"))).unwrap();
    let err = camera.connect().unwrap_err();
    assert_eq!(err.code, ErrorCode::Io);
    assert!(err.message.contains("camera reported an error"));
    assert_eq!(err.details.as_deref(), Some("no context"));
}

#[test]
fn a_picture_that_was_never_collected_is_dropped_by_the_next_trigger() {
    let script = Arc::new(Script::default());
    script.push_capture(Step::File(file(1)));
    script.push_capture(Step::File(file(2)));
    let mut camera = camera(&script);
    camera.arm().unwrap();
    camera.trigger().unwrap();
    camera.trigger().unwrap();
    assert_eq!(
        camera.wait_for_image(Duration::from_secs(5)).unwrap(),
        file(2)
    );
}

#[test]
fn waiting_without_a_trigger_is_an_internal_error() {
    let script = Arc::new(Script::default());
    let mut camera = camera(&script);
    assert_eq!(
        camera.wait_for_image(SHORT).unwrap_err().code,
        ErrorCode::Internal
    );
}

#[test]
fn types_that_cross_threads_are_send() {
    fn send<T: Send>() {}
    fn send_sync<T: Send + Sync>() {}
    send::<GphotoCamera>();
    send::<Box<dyn CameraSource + Send>>();
    send_sync::<super::worker::GphotoFinder>();
}

// --- the error words ----------------------------------------------------------------------------

#[test]
fn the_usb_claim_error_names_the_workaround_and_keeps_the_raw_text_as_details() {
    let raw =
        "Could not claim the USB device [Could not claim interface 0 (No such file or directory)]";
    // Recognised by the code the binding gives ...
    let by_kind = SessionError::new(SessionErrorKind::Claim, "x").into_app_error();
    // ... and by the message alone, whatever code it came with.
    let by_text = SessionError::new(SessionErrorKind::Other, raw).into_app_error();
    for err in [by_kind, by_text.clone()] {
        assert_eq!(err.code, ErrorCode::Io);
        let m = &err.message;
        assert!(m.contains("Log out any other user accounts"), "{m}");
        assert!(m.contains("Photos") && m.contains("Image Capture"), "{m}");
        assert!(
            !m.to_lowercase().contains(CLAIM_MARKER),
            "no raw error in the words"
        );
    }
    assert_eq!(by_text.details.as_deref(), Some(raw));
    assert!(!by_text.message.contains("sudo") && !by_text.message.contains("pkill"));
}

#[test]
fn a_claim_failure_reaches_the_user_through_connect_arm_and_capture() {
    let script = Arc::new(Script::default());
    let claim = || {
        Err(SessionError::new(
            SessionErrorKind::Other,
            "Could not claim the USB device",
        ))
    };
    script.opens.lock().unwrap().extend([claim(), claim()]);
    let mut camera = camera(&script);
    assert!(camera.connect().unwrap_err().message.contains("Log out"));
    assert!(camera.arm().unwrap_err().message.contains("Photos"));
    assert!(!camera.is_connected());
    assert!(
        !camera.is_unusable(),
        "a claim failure is not a hung camera"
    );
    // Freed, the camera opens at the next attempt.
    camera.connect().unwrap();
}

#[test]
fn session_errors_are_mapped_to_codes_and_words() {
    let cases = [
        (
            SessionErrorKind::NoCamera,
            ErrorCode::NotFound,
            "No camera was found",
        ),
        (SessionErrorKind::Busy, ErrorCode::Io, "camera is busy"),
        (
            SessionErrorKind::Lost,
            ErrorCode::Io,
            "connection to the camera was lost",
        ),
        (
            SessionErrorKind::Other,
            ErrorCode::Io,
            "camera reported an error",
        ),
    ];
    for (kind, code, words) in cases {
        let err = SessionError::new(kind, "raw text").into_app_error();
        assert_eq!(err.code, code, "{kind:?}");
        assert!(err.message.contains(words), "{kind:?}: {}", err.message);
        assert_eq!(err.details.as_deref(), Some("raw text"));
        assert!(!err.message.contains("raw text"));
    }
}

#[test]
fn a_lost_link_closes_the_session_and_the_next_arm_opens_it_again() {
    let script = Arc::new(Script::default());
    script.push_capture(Step::Fail(SessionError::new(SessionErrorKind::Lost, "usb")));
    script.push_capture(Step::File(file(2)));
    let mut camera = camera(&script);
    let err = shoot(&mut camera, Duration::from_secs(5)).unwrap_err();
    assert!(err.message.contains("connection to the camera was lost"));
    eventually(|| !camera.is_connected());
    assert!(!camera.is_unusable(), "a lost link is not a hung camera");
    assert_eq!(shoot(&mut camera, Duration::from_secs(5)).unwrap(), file(2));
    assert_eq!(
        script.log(),
        ["open", "capture", "close", "open", "capture"]
    );
}

#[test]
fn a_failed_open_during_a_slow_start_times_out_and_blocks_further_use() {
    let script = Arc::new(Script::default());
    let gate = Arc::new(Gate::default());
    let slow = Arc::clone(&gate);
    let factory_script = Arc::clone(&script);
    let camera = GphotoCamera::with_timeouts(
        move || {
            // The session cannot be built until the gate opens: a hung libgphoto2 at start.
            slow.wait();
            Ok(Box::new(Scripted(Arc::clone(&factory_script))) as Box<dyn Session>)
        },
        Timeouts {
            open: SHORT,
            list: SHORT,
            settings: SHORT,
            preview: SHORT,
        },
    )
    .unwrap();
    assert!(camera
        .connect()
        .unwrap_err()
        .message
        .contains("did not answer"));
    assert!(camera.is_unusable());
    gate.release();
}

// --- settings -----------------------------------------------------------------------------------

fn menu(kind: CameraSettingKind, value: &str, choices: &[&str], writable: bool) -> CameraSetting {
    CameraSetting::from_camera(
        kind,
        value,
        choices.iter().map(|c| (*c).to_owned()),
        writable,
    )
}

fn d600_settings() -> Vec<CameraSetting> {
    vec![
        menu(CameraSettingKind::Iso, "100", &["100", "200", "400"], true),
        menu(
            CameraSettingKind::ShutterSpeed,
            "1/100",
            &["1/60", "1/100", "1/125"],
            false,
        ),
        CameraSetting::absent(CameraSettingKind::Aperture),
    ]
}

#[test]
fn the_settings_are_read_from_the_session_with_the_camera_opened_first() {
    let script = Arc::new(Script::default().with_settings(d600_settings()));
    let camera = camera(&script);
    let got = camera.finder().settings().unwrap();
    assert_eq!(got, d600_settings());
    assert_eq!(script.log(), ["open", "settings"]);
    assert!(camera.is_connected());
}

#[test]
fn a_valid_value_is_set_and_the_reply_is_the_settings_read_again() {
    let script = Arc::new(Script::default().with_settings(d600_settings()));
    let camera = camera(&script);
    let after = camera
        .finder()
        .set_setting(CameraSettingKind::Iso, " 400 ")
        .unwrap();
    assert_eq!(after[0].value.as_deref(), Some("400"));
    assert_eq!(
        script.log(),
        ["open", "settings", "set Iso 400", "settings"]
    );
}

#[test]
fn a_value_that_is_not_a_choice_is_refused_before_anything_is_written() {
    let script = Arc::new(Script::default().with_settings(d600_settings()));
    let camera = camera(&script);
    let err = camera
        .finder()
        .set_setting(CameraSettingKind::Iso, "300")
        .unwrap_err();
    assert!(err.message.contains("not one the camera offers"), "{err}");
    assert_eq!(script.log(), ["open", "settings"]);
}

#[test]
fn a_readonly_or_absent_setting_is_refused_in_words() {
    let script = Arc::new(Script::default().with_settings(d600_settings()));
    let finder = camera(&script).finder();
    let readonly = finder
        .set_setting(CameraSettingKind::ShutterSpeed, "1/60")
        .unwrap_err();
    assert!(readonly.message.contains("M mode"), "{readonly}");
    let absent = finder
        .set_setting(CameraSettingKind::Aperture, "8")
        .unwrap_err();
    assert!(absent.message.contains("does not have"), "{absent}");
    assert_eq!(script.count("settings"), 2);
    assert!(script.log().iter().all(|e| !e.starts_with("set ")));
}

#[test]
fn settings_are_refused_while_a_picture_is_being_taken() {
    let script = Arc::new(Script::default().with_settings(d600_settings()));
    let gate = Arc::new(Gate::default());
    script.push_capture(Step::Hold(Arc::clone(&gate), file(1)));
    let mut camera = camera(&script);
    let finder = camera.finder();
    camera.arm().unwrap();
    camera.trigger().unwrap();
    for err in [
        finder.settings().unwrap_err(),
        finder
            .set_setting(CameraSettingKind::Iso, "200")
            .unwrap_err(),
    ] {
        assert!(err.message.contains("being taken"), "{}", err.message);
    }
    assert!(!camera.is_unusable());
    gate.release();
    camera.wait_for_image(Duration::from_secs(5)).unwrap();
    assert_eq!(finder.settings().unwrap(), d600_settings());
}

#[test]
fn a_settings_call_that_does_not_answer_makes_the_camera_unusable_like_any_other() {
    let script = Arc::new(Script::default().with_settings(d600_settings()));
    let gate = Arc::new(Gate::default());
    *script
        .settings_gate
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&gate));
    let camera = camera_with(
        &script,
        Timeouts {
            open: Duration::from_secs(5),
            list: Duration::from_secs(5),
            settings: SHORT,
            preview: SHORT,
        },
    );
    let finder = camera.finder();
    let err = finder.settings().unwrap_err();
    assert!(err.message.contains("did not answer"), "{err}");
    assert!(camera.is_unusable());
    assert!(finder
        .settings()
        .unwrap_err()
        .message
        .contains("stopped answering"));
    gate.release();
}

#[test]
fn a_lost_link_while_reading_settings_closes_the_session_for_the_next_call() {
    let script = Arc::new(Script::default().with_settings(d600_settings()));
    *script
        .settings_error
        .lock()
        .unwrap_or_else(PoisonError::into_inner) =
        Some(SessionError::new(SessionErrorKind::Lost, "usb"));
    let camera = camera(&script);
    let finder = camera.finder();
    let err = finder.settings().unwrap_err();
    assert!(err.message.contains("connection to the camera was lost"));
    eventually(|| !camera.is_connected());
    assert!(!camera.is_unusable());
    assert_eq!(finder.settings().unwrap(), d600_settings());
    assert_eq!(
        script.log(),
        ["open", "settings", "close", "open", "settings"]
    );
}

// --- live view ----------------------------------------------------------------------------------

#[test]
fn frames_come_from_the_session_with_the_camera_opened_first() {
    let script = Arc::new(Script::default());
    let camera = camera(&script);
    let finder = camera.finder();
    assert_eq!(finder.preview_frame().unwrap(), [0xFF, 0xD8, 0xFF, 0xD9]);
    finder.preview_frame().unwrap();
    assert_eq!(script.log(), ["open", "preview", "preview"]);
    assert!(camera.is_connected());
}

#[test]
fn live_view_is_left_once_before_the_next_capture() {
    let script = Arc::new(Script::default());
    script.push_capture(Step::File(file(1)));
    script.push_capture(Step::File(file(2)));
    let mut camera = camera(&script);
    let finder = camera.finder();
    finder.preview_frame().unwrap();
    finder.preview_frame().unwrap();
    shoot(&mut camera, SHORT * 100).unwrap();
    shoot(&mut camera, SHORT * 100).unwrap();
    assert_eq!(
        script.log(),
        ["open", "preview", "preview", "leave", "capture", "capture"]
    );
}

#[test]
fn a_capture_without_live_view_does_not_touch_it() {
    let script = Arc::new(Script::default());
    script.push_capture(Step::File(file(1)));
    let mut camera = camera(&script);
    shoot(&mut camera, SHORT * 100).unwrap();
    assert_eq!(script.log(), ["open", "capture"]);
}

#[test]
fn a_stop_leaves_live_view_only_when_it_is_on() {
    let script = Arc::new(Script::default());
    script.push_capture(Step::File(file(1)));
    let mut camera = camera(&script);
    let finder = camera.finder();
    finder.preview_stop().unwrap();
    assert!(script.log().is_empty(), "nothing to leave, nothing asked");
    finder.preview_frame().unwrap();
    finder.preview_stop().unwrap();
    finder.preview_stop().unwrap();
    shoot(&mut camera, SHORT * 100).unwrap();
    assert_eq!(script.log(), ["open", "preview", "leave", "capture"]);
}

#[test]
fn a_stop_that_fails_is_tried_again_and_a_capture_still_goes_ahead_when_leaving_fails() {
    let script = Arc::new(Script::default());
    script.push_capture(Step::File(file(1)));
    let mut camera = camera(&script);
    let finder = camera.finder();
    finder.preview_frame().unwrap();
    *script
        .leave_error
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(other("viewfinder"));
    assert!(finder.preview_stop().is_err());
    finder.preview_stop().unwrap();
    assert_eq!(script.count("leave"), 2);

    finder.preview_frame().unwrap();
    *script
        .leave_error
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(other("viewfinder"));
    assert_eq!(
        shoot(&mut camera, SHORT * 100).unwrap().name,
        "DSC_0001.NEF"
    );
}

#[test]
fn frames_and_stops_are_refused_while_a_picture_is_being_taken() {
    let script = Arc::new(Script::default());
    let gate = Arc::new(Gate::default());
    script.push_capture(Step::Hold(Arc::clone(&gate), file(1)));
    let mut camera = camera(&script);
    let finder = camera.finder();
    camera.arm().unwrap();
    camera.trigger().unwrap();
    for err in [
        finder.preview_frame().unwrap_err(),
        finder.preview_stop().unwrap_err(),
    ] {
        assert!(err.message.contains("being taken"), "{}", err.message);
    }
    assert!(!camera.is_unusable());
    gate.release();
    camera.wait_for_image(Duration::from_secs(5)).unwrap();
    finder.preview_frame().unwrap();
}

#[test]
fn a_lost_link_during_live_view_means_the_new_session_is_not_left() {
    let script = Arc::new(Script::default());
    script.push_capture(Step::File(file(1)));
    let mut camera = camera(&script);
    let finder = camera.finder();
    finder.preview_frame().unwrap();
    *script
        .preview_error
        .lock()
        .unwrap_or_else(PoisonError::into_inner) =
        Some(SessionError::new(SessionErrorKind::Lost, "usb"));
    let err = finder.preview_frame().unwrap_err();
    assert!(err.message.contains("connection to the camera was lost"));
    eventually(|| !camera.is_connected());
    shoot(&mut camera, SHORT * 100).unwrap();
    assert_eq!(
        script.log(),
        ["open", "preview", "preview", "close", "open", "capture"]
    );
}

#[test]
fn a_preview_call_that_does_not_answer_makes_the_camera_unusable() {
    let script = Arc::new(Script::default());
    let gate = Arc::new(Gate::default());
    let slow = Arc::clone(&gate);
    let factory_script = Arc::clone(&script);
    let camera = GphotoCamera::with_timeouts(
        move || {
            slow.wait();
            Ok(Box::new(Scripted(Arc::clone(&factory_script))) as Box<dyn Session>)
        },
        Timeouts {
            open: Duration::from_secs(5),
            list: Duration::from_secs(5),
            settings: Duration::from_secs(5),
            preview: SHORT,
        },
    )
    .unwrap();
    let err = camera.finder().preview_frame().unwrap_err();
    assert!(err.message.contains("did not answer"), "{err}");
    assert!(camera.is_unusable());
    gate.release();
}

// --- library paths ------------------------------------------------------------------------------

#[test]
fn the_bundle_layout_is_found_from_the_binary_and_a_dev_build_has_none() {
    let exe = Path::new("/Applications/Grain to Pixel.app/Contents/MacOS/grain-to-pixel-desktop");
    let bundled =
        |p: &Path| p.starts_with("/Applications/Grain to Pixel.app/Contents/Frameworks/gphoto");
    let dirs = bundled_library_dirs(exe, bundled).unwrap();
    assert!(dirs.camlibs.ends_with("Frameworks/gphoto/camlibs"));
    assert!(dirs.iolibs.ends_with("Frameworks/gphoto/iolibs"));
    // Not a bundle: no folders, nothing to set (libgphoto2 keeps its Homebrew paths).
    assert_eq!(
        bundled_library_dirs(
            Path::new("/repo/target/debug/grain-to-pixel-desktop"),
            |_| false
        ),
        None
    );
    // Only one of the two folders is not enough.
    assert_eq!(bundled_library_dirs(exe, |p| p.ends_with("camlibs")), None);
}

// --- conformance: the same scenarios through the trait, for the fake and for the worker -----------

/// What a rig's camera does at each shot.
#[derive(Clone)]
enum Outcome {
    File(CapturedFile),
    Fail,
    Hang,
}

struct Built {
    camera: Box<dyn CameraSource>,
    /// Frees whatever a hung shot is holding.
    release: Box<dyn FnOnce()>,
}

type Rig = fn(&[Outcome]) -> Built;

fn fake_rig(outcomes: &[Outcome]) -> Built {
    let shots = outcomes
        .iter()
        .map(|o| match o {
            Outcome::File(f) => Shot::File(f.clone()),
            Outcome::Fail | Outcome::Hang => Shot::Fail(no_picture()),
        })
        .collect();
    Built {
        camera: Box::new(FakeCamera::new(shots)),
        release: Box::new(|| {}),
    }
}

fn worker_rig(outcomes: &[Outcome]) -> Built {
    let script = Arc::new(Script::default());
    let gate = Arc::new(Gate::default());
    for outcome in outcomes {
        script.push_capture(match outcome {
            Outcome::File(f) => Step::File(f.clone()),
            Outcome::Fail => Step::Fail(other("the camera said no")),
            Outcome::Hang => Step::Hold(Arc::clone(&gate), file(99)),
        });
    }
    let camera = camera(&script);
    // The sequence checks the link before it arms: the app connects first.
    camera.connect().unwrap();
    Built {
        camera: Box::new(camera),
        release: Box::new(move || gate.release()),
    }
}

const RIGS: [(&str, Rig); 2] = [("FakeCamera", fake_rig), ("GphotoCamera", worker_rig)];

struct NoPause;

impl Pause for NoPause {
    fn pause(&self, _: Duration) {}
}

#[test]
fn conformance_pictures_arrive_in_order_through_the_trait() {
    for (name, rig) in RIGS {
        let mut built = rig(&[Outcome::File(file(1)), Outcome::File(file(2))]);
        assert!(built.camera.is_connected(), "{name}");
        for n in 1..=2 {
            let got = shoot(&mut *built.camera, Duration::from_secs(5)).unwrap();
            assert_eq!(got, file(n), "{name}");
        }
        (built.release)();
    }
}

#[test]
fn conformance_a_failed_shot_is_an_error_and_the_next_shot_still_works() {
    for (name, rig) in RIGS {
        let mut built = rig(&[Outcome::Fail, Outcome::File(file(2))]);
        assert!(
            shoot(&mut *built.camera, Duration::from_secs(5)).is_err(),
            "{name}"
        );
        assert_eq!(
            shoot(&mut *built.camera, Duration::from_secs(5)).unwrap(),
            file(2),
            "{name}"
        );
        (built.release)();
    }
}

#[test]
fn conformance_a_full_capture_sequence_takes_r_g_b_and_leaves_the_light_off() {
    for (name, rig) in RIGS {
        let mut built = rig(&[
            Outcome::File(file(1)),
            Outcome::File(file(2)),
            Outcome::File(file(3)),
        ]);
        let mut light = FakeLight::new();
        let handle = light.handle();
        let sequence = CaptureSequence::with_pause(CaptureProfile::default(), &NoPause);
        let outcome = sequence
            .run(&mut light, &mut *built.camera, &|| false, &mut |_| {})
            .unwrap();
        assert_eq!(outcome.files, [file(1), file(2), file(3)], "{name}");
        let levels: Vec<[u8; 3]> = handle
            .commands()
            .into_iter()
            .filter_map(|c| match c {
                LightCommand::SetLevels(l) => Some(l),
                _ => None,
            })
            .collect();
        assert_eq!(
            levels,
            [[44, 0, 0], [0, 22, 0], [0, 0, 2], [0, 0, 0]],
            "{name}: one colour at a time, then off"
        );
        assert_eq!(handle.levels(), [0; 3], "{name}");
        (built.release)();
    }
}

#[test]
fn conformance_a_camera_that_never_answers_ends_the_sequence_with_the_light_off() {
    for (name, rig) in RIGS {
        let mut built = rig(&[Outcome::File(file(1)), Outcome::Hang]);
        let mut light = FakeLight::new();
        let handle = light.handle();
        let sequence =
            CaptureSequence::with_pause(CaptureProfile::default(), &NoPause).capture_timeout(SHORT);
        let err = sequence
            .run(&mut light, &mut *built.camera, &|| false, &mut |_| {})
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Io, "{name}");
        assert_eq!(
            handle.levels(),
            [0; 3],
            "{name}: the guard switched the light off"
        );
        (built.release)();
    }
}

#[test]
fn the_worker_after_a_hung_shot_refuses_the_next_sequence_before_lighting_anything() {
    let mut built = worker_rig(&[Outcome::Hang]);
    let mut light = FakeLight::new();
    let handle = light.handle();
    let sequence =
        CaptureSequence::with_pause(CaptureProfile::default(), &NoPause).capture_timeout(SHORT);
    assert!(sequence
        .run(&mut light, &mut *built.camera, &|| false, &mut |_| {})
        .is_err());
    let before = handle.commands().len();
    let err = sequence
        .run(&mut light, &mut *built.camera, &|| false, &mut |_| {})
        .unwrap_err();
    assert!(err.message.contains("not connected"), "{}", err.message);
    let lit = handle.commands()[before..]
        .iter()
        .any(|c| matches!(c, LightCommand::SetLevels(l) if *l != [0; 3]));
    assert!(!lit, "the light stays dark when the camera is unusable");
    (built.release)();
}
