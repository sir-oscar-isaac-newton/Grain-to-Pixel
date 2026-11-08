//! Capture commands: the light, the camera, the capture profile, the test shot and the capture of
//! one frame (docs/specification.md section 6.1). Thin: each one calls a `gtp-app` service.

use std::sync::Arc;

use gtp_app::services::capture_control as capture;
use gtp_app::services::capture_levels::{self, CaptureLevels, LevelsScope};
use gtp_app::{
    AppContext, CameraInfo, CaptureProfile, CaptureProfilePatch, Hardware, JobManager, LightInfo,
    PortInfo, TestShot,
};
use gtp_core::{ExposureInfo, JobId, RollId, TripletChannel};
use tauri::State;

use crate::error::CmdResult;
use crate::sink::{CaptureEventsState, EventsState};

/// The serial ports the Scanlight may be on.
#[tauri::command(async)]
#[specta::specta]
pub fn light_list(hardware: State<'_, Hardware>) -> CmdResult<Vec<PortInfo>> {
    capture::light_list(&hardware)
}

/// Connects the light on `port`, or finds it by probing the serial ports. Refused while a capture
/// is in progress; clears a temperature trip.
#[tauri::command(async)]
#[specta::specta]
pub fn light_connect(hardware: State<'_, Hardware>, port: Option<String>) -> CmdResult<LightInfo> {
    capture::light_connect(&hardware, port.as_deref())
}

/// Lights red, green and blue at these levels (0 to 255). Refused while a capture is in progress,
/// after a temperature trip, and when the light is not connected, reports no current temperature
/// or is over the abort limit (switching off is always allowed).
#[tauri::command(async)]
#[specta::specta]
pub fn light_set(
    ctx: State<'_, AppContext>,
    hardware: State<'_, Hardware>,
    levels: [u8; 3],
) -> CmdResult<()> {
    capture::light_set(
        &hardware,
        levels,
        ctx.settings().capture_profile().temp_abort_c,
    )
}

/// Switches the light off. Always allowed.
#[tauri::command(async)]
#[specta::specta]
pub fn light_off(hardware: State<'_, Hardware>) -> CmdResult<()> {
    capture::light_off(&hardware)
}

/// The cameras that can be used (none until a camera backend exists, except with `GTP_FAKE_HW=1`).
#[tauri::command(async)]
#[specta::specta]
pub fn camera_list(hardware: State<'_, Hardware>) -> CmdResult<Vec<CameraInfo>> {
    Ok(capture::camera_list(&hardware))
}

/// The capture profile (levels, settle time, temperature limits, camera backend).
#[tauri::command(async)]
#[specta::specta]
pub fn capture_profile_get(ctx: State<'_, AppContext>) -> CmdResult<CaptureProfile> {
    Ok(ctx.settings().capture_profile())
}

/// Changes the given fields of the profile, validates it as a whole, saves it in `settings.json`
/// and returns it.
#[tauri::command(async)]
#[specta::specta]
pub fn capture_profile_set(
    ctx: State<'_, AppContext>,
    patch: CaptureProfilePatch,
) -> CmdResult<CaptureProfile> {
    ctx.settings().update_capture(&patch)
}

/// The levels the roll captures with: saved for its film stock, else the global default, with the
/// camera settings they were saved after (H-15).
#[tauri::command(async)]
#[specta::specta]
pub fn capture_levels_get(ctx: State<'_, AppContext>, roll_id: RollId) -> CmdResult<CaptureLevels> {
    capture_levels::levels_for_roll(&ctx, roll_id)
}

/// Saves `levels` (0 to 255 each) as the global default or for a film stock, with the camera
/// settings of the test shot they were balanced on. Returns the levels as they now resolve for that
/// scope.
#[tauri::command(async)]
#[specta::specta]
pub fn capture_levels_save(
    ctx: State<'_, AppContext>,
    scope: LevelsScope,
    levels: [u32; 3],
    exposure: Option<ExposureInfo>,
) -> CmdResult<CaptureLevels> {
    capture_levels::save_levels(&ctx, &scope, levels, exposure)
}

/// Lights one colour at its level of `levels` (the working levels, 0 to 255 each), takes a
/// picture, and returns the histogram and clipped percentage of that channel and the picture's
/// camera settings. Stores nothing; the light is off afterwards.
#[tauri::command(async)]
#[specta::specta]
pub fn capture_test(
    ctx: State<'_, AppContext>,
    hardware: State<'_, Hardware>,
    events: State<'_, CaptureEventsState>,
    channel: TripletChannel,
    levels: [u32; 3],
) -> CmdResult<TestShot> {
    capture::test_shot(&ctx, &hardware, events.0.as_ref(), channel, levels)
}

/// Captures one frame into the roll as a job; the new frame arrives in `job://done` like an
/// import's, the steps as `capture://step`. `levels` are the working levels (0 to 255 each). Returns
/// the job id at once.
#[tauri::command(async)]
#[specta::specta]
pub fn capture_triplet(
    ctx: State<'_, AppContext>,
    jobs: State<'_, JobManager>,
    hardware: State<'_, Hardware>,
    library_events: State<'_, EventsState>,
    events: State<'_, CaptureEventsState>,
    roll_id: RollId,
    levels: [u32; 3],
) -> CmdResult<JobId> {
    capture::start_triplet(
        &ctx,
        &jobs,
        &hardware,
        Arc::clone(&library_events.0),
        Arc::clone(&events.0),
        roll_id,
        levels,
    )
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::mpsc;
    use std::time::Duration;

    use gtp_core::ErrorCode;
    use tauri::test::{mock_app, MockRuntime};
    use tauri::{App, Listener, Manager};

    use super::*;
    use crate::sink::TauriJobSink;

    const WAIT: Duration = Duration::from_secs(30);

    /// A mock app set up as the real one is with `GTP_FAKE_HW=1`: fake light and camera, a library
    /// with one roll, and the typed events mounted.
    struct Fixture {
        app: App<MockRuntime>,
        roll: RollId,
        dir: std::path::PathBuf,
    }

    fn fixture(label: &str) -> Fixture {
        let app = mock_app();
        crate::ipc::builder::<MockRuntime>().mount_events(&app);
        let dir = std::env::temp_dir().join(format!(
            "gtp-capture-commands-{label}-{}-{}",
            std::process::id(),
            gtp_core::FrameId::new()
        ));
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        ctx.create_library(&dir.join("lib"), "Capture").unwrap();
        let roll = gtp_app::services::roll_frame::roll_create(
            &ctx.library().unwrap(),
            gtp_app::services::roll_frame::RollInput {
                name: "Camera".into(),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
        ctx.settings()
            .update_capture(&CaptureProfilePatch {
                settle_ms: Some(0),
                ..CaptureProfilePatch::default()
            })
            .unwrap();
        let sink = Arc::new(TauriJobSink::new(app.handle().clone()));
        app.manage(JobManager::new(sink.clone()));
        app.manage(CaptureEventsState(sink.clone()));
        app.manage(EventsState(sink));
        app.manage(ctx);
        app.manage(Hardware::from_vars(Some("1"), Some(OsString::from("none"))));
        Fixture { app, roll, dir }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn listen(
        app: &App<MockRuntime>,
        names: &[&'static str],
    ) -> mpsc::Receiver<(String, serde_json::Value)> {
        let (tx, rx) = mpsc::channel();
        for name in names.iter().copied() {
            let tx = tx.clone();
            app.listen_any(name, move |event| {
                let payload = serde_json::from_str(event.payload()).unwrap();
                let _ = tx.send((name.to_owned(), payload));
            });
        }
        rx
    }

    #[test]
    fn the_light_and_camera_commands_answer_on_the_fakes() {
        let f = fixture("light");
        let hardware = || f.app.state::<Hardware>();
        assert_eq!(light_list(hardware()).unwrap()[0].name, "fake");
        assert_eq!(camera_list(hardware()).unwrap()[0].id, "fake");
        assert_eq!(light_connect(hardware(), None).unwrap().port, "fake");
        light_set(f.app.state(), hardware(), [10, 20, 30]).unwrap();
        assert!(!hardware().light.is_tripped());
        light_off(hardware()).unwrap();
    }

    #[test]
    fn the_profile_is_read_changed_validated_and_kept() {
        let f = fixture("profile");
        let ctx = || f.app.state::<AppContext>();
        let first = capture_profile_get(ctx()).unwrap();
        assert_eq!(first.levels, [44, 22, 2]);
        let patch = CaptureProfilePatch {
            levels: Some([90, 40, 5]),
            ..CaptureProfilePatch::default()
        };
        let set = capture_profile_set(ctx(), patch).unwrap();
        assert_eq!(set.levels, [90, 40, 5]);
        assert_eq!(set.settle_ms, 0, "the other fields stay");
        assert_eq!(capture_profile_get(ctx()).unwrap(), set);
        let text = std::fs::read_to_string(f.dir.join("settings.json")).unwrap();
        assert!(text.contains("\"capture\""), "{text}");

        for bad in [
            CaptureProfilePatch {
                levels: Some([256, 0, 0]),
                ..CaptureProfilePatch::default()
            },
            CaptureProfilePatch {
                temp_warn_c: Some(60.0),
                ..CaptureProfilePatch::default()
            },
        ] {
            let err = capture_profile_set(ctx(), bad).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
        }
        assert_eq!(capture_profile_get(ctx()).unwrap(), set);
    }

    #[test]
    fn a_test_shot_returns_its_measurements_and_emits_the_steps() {
        let f = fixture("test-shot");
        let rx = listen(&f.app, &["capture://step"]);
        let shot = capture_test(
            f.app.state(),
            f.app.state(),
            f.app.state(),
            TripletChannel::Green,
            [44, 22, 2],
        )
        .unwrap();
        assert_eq!(shot.histogram.len(), 256);
        assert_eq!(shot.clipped_pct, 0.0);
        let states: Vec<_> = (0..3)
            .map(|_| rx.recv_timeout(WAIT).unwrap().1)
            .map(|p| (p["channel"].clone(), p["state"].clone()))
            .collect();
        assert_eq!(
            states,
            [
                ("green".into(), "lit".into()),
                ("green".into(), "fired".into()),
                ("green".into(), "received".into())
            ]
        );
    }

    #[test]
    fn levels_are_resolved_per_roll_saved_with_their_scope_and_validated() {
        use gtp_app::services::capture_levels::{LevelsScope, LevelsSource};

        let f = fixture("levels");
        let got = capture_levels_get(f.app.state(), f.roll).unwrap();
        assert_eq!(got.levels, [44, 22, 2]);
        assert_eq!(got.source, LevelsSource::Default);

        let saved = capture_levels_save(
            f.app.state(),
            LevelsScope::Stock("Portra 400".into()),
            [60, 30, 4],
            None,
        )
        .unwrap();
        assert_eq!(saved.source, LevelsSource::Stock);
        // The roll has no film stock, so it still uses the default.
        assert_eq!(
            capture_levels_get(f.app.state(), f.roll).unwrap().levels,
            [44, 22, 2]
        );
        let err = capture_levels_save(f.app.state(), LevelsScope::Default, [1, 2, 256], None)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let text = std::fs::read_to_string(f.dir.join("settings.json")).unwrap();
        assert!(text.contains("portra 400"), "{text}");
        // The scope reaches the command as the UI sends it.
        assert_eq!(
            serde_json::from_str::<LevelsScope>("\"default\"").unwrap(),
            LevelsScope::Default
        );
        assert_eq!(
            serde_json::from_str::<LevelsScope>("{\"stock\":\"HP5\"}").unwrap(),
            LevelsScope::Stock("HP5".into())
        );
    }

    #[test]
    fn a_capture_returns_a_job_that_ends_with_the_new_frame() {
        let f = fixture("triplet");
        let rx = listen(&f.app, &["job://done", "job://error", "capture://step"]);
        let id = capture_triplet(
            f.app.state(),
            f.app.state(),
            f.app.state(),
            f.app.state(),
            f.app.state(),
            f.roll,
            [44, 22, 2],
        )
        .unwrap();
        let mut steps = 0;
        let done = loop {
            let (name, payload) = rx.recv_timeout(WAIT).unwrap();
            match name.as_str() {
                "capture://step" => steps += 1,
                "job://done" => break payload,
                other => panic!("{other}: {payload}"),
            }
        };
        assert_eq!(done["id"], id.to_string());
        assert_eq!(done["result"]["kind"], "Imported");
        assert_eq!(done["result"]["frames"].as_array().map(Vec::len), Some(1));
        assert_eq!(steps, 9);
    }

    #[test]
    fn without_a_camera_backend_a_capture_and_a_test_shot_say_so() {
        let f = fixture("no-camera");
        let real = Hardware::from_vars(None, None);
        let ctx = f.app.state::<AppContext>();
        let events = f.app.state::<CaptureEventsState>();
        let err = capture::test_shot(
            &ctx,
            &real,
            events.0.as_ref(),
            TripletChannel::Red,
            [44, 22, 2],
        )
        .unwrap_err();
        assert!(err.message.contains("No camera backend"), "{err}");
        assert!(capture::camera_list(&real).is_empty());
    }
}
