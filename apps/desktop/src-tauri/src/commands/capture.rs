//! Capture commands: the light, the camera, the capture profile, the test shot and the capture of
//! one frame (docs/specification.md section 6.1). Thin: each one calls a `gtp-app` service.

use std::sync::Arc;

use gtp_app::services::capture_control as capture;
use gtp_app::services::capture_levels::{self, CaptureLevels, LevelsScope};
use gtp_app::{
    AppContext, CameraInfo, CameraSetting, CameraSettingKind, CaptureProfile, CaptureProfilePatch,
    FocusCheck, Hardware, JobManager, LightInfo, PortInfo, Region, TestShot, ZoomStep,
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

/// The camera's ISO, shutter speed and aperture with the choices it allows. A setting the camera
/// lacks comes back absent (no value, no choices, not writable). Refused while a capture or a
/// test shot runs.
#[tauri::command(async)]
#[specta::specta]
pub fn camera_settings_get(hardware: State<'_, Hardware>) -> CmdResult<Vec<CameraSetting>> {
    capture::camera_settings(&hardware)
}

/// Sets one camera setting to one of its choices and returns the settings as the camera has them
/// afterwards. Refused while a capture or a test shot runs, and for a value the camera does not
/// offer or a setting it does not let change now.
#[tauri::command(async)]
#[specta::specta]
pub fn camera_settings_set(
    hardware: State<'_, Hardware>,
    kind: CameraSettingKind,
    value: String,
) -> CmdResult<Vec<CameraSetting>> {
    capture::camera_set_setting(&hardware, kind, &value)
}

/// One live-view frame of the camera as a JPEG, in raw bytes and not JSON (about 7 KB, up to 30
/// times a second); the first call puts the camera in live view. The light is not touched. Refused
/// while a capture, a test shot or a calibration runs. Not in the typed bindings (see
/// `ipc::RAW_COMMANDS`).
#[tauri::command(async)]
pub fn camera_preview_frame(hardware: State<'_, Hardware>) -> CmdResult<tauri::ipc::Response> {
    let bytes = capture::camera_preview_frame(&hardware)?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Leaves the camera's live view; does nothing when it is not in live view or while a capture runs
/// (a capture leaves live view itself).
#[tauri::command(async)]
#[specta::specta]
pub fn camera_preview_stop(hardware: State<'_, Hardware>) -> CmdResult<()> {
    capture::camera_preview_stop(&hardware)
}

/// The zoom steps of the camera's live view, whole frame first (`value` is what to set, `label`
/// what to show); empty when the camera has no zoom. Refused while a capture runs.
#[tauri::command(async)]
#[specta::specta]
pub fn camera_preview_zoom_steps(hardware: State<'_, Hardware>) -> CmdResult<Vec<ZoomStep>> {
    capture::camera_preview_zoom_steps(&hardware)
}

/// Sets the live-view zoom to one of the steps (anything else is refused), putting the camera in
/// live view if it is not in it. The zoom goes back to the whole frame when live view is left.
#[tauri::command(async)]
#[specta::specta]
pub fn camera_preview_set_zoom(hardware: State<'_, Hardware>, value: String) -> CmdResult<()> {
    capture::camera_preview_set_zoom(&hardware, &value)
}

/// Moves the zoomed area to the point (`x`, `y`) of the frame, each a fraction from 0 to 1.
#[tauri::command(async)]
#[specta::specta]
pub fn camera_preview_set_area(hardware: State<'_, Hardware>, x: f32, y: f32) -> CmdResult<()> {
    capture::camera_preview_set_area(&hardware, x, y)
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

/// The focus check (H-19): lights one colour at its level of `levels` (the working levels, 0 to 255
/// each), takes a picture and returns a 1:1 crop of the middle of that colour (a base64 JPEG), its
/// sharpness number and a 3 x 3 map of sharpness over the frame with the pattern it shows (flat,
/// bowed, tilted), against the saved flat reference when there is one. Stores nothing; the light
/// is off afterwards; refused while a capture runs. The reference is saved and cleared with
/// `settings_set` (`focus_reference`, `clear_focus_reference`).
#[tauri::command(async)]
#[specta::specta]
pub fn capture_focus_check(
    ctx: State<'_, AppContext>,
    hardware: State<'_, Hardware>,
    events: State<'_, CaptureEventsState>,
    channel: TripletChannel,
    levels: [u32; 3],
) -> CmdResult<FocusCheck> {
    capture::focus_check(&ctx, &hardware, events.0.as_ref(), channel, levels)
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

/// Finds the Scanlight levels automatically as a job: test shots, one colour at a time, until the
/// brightest pixels of each sit just below clipping. `levels` are the working levels it starts from
/// (0 to 255 each); `region` is the part of the frame it measures, as fractions of its width and
/// height (the whole frame when absent). The result arrives in `job://done` as `Calibrated`, the
/// steps as `capture://step`. Nothing is saved: the user saves the levels. Returns the job id at once.
#[tauri::command(async)]
#[specta::specta]
pub fn capture_calibrate(
    ctx: State<'_, AppContext>,
    jobs: State<'_, JobManager>,
    hardware: State<'_, Hardware>,
    events: State<'_, CaptureEventsState>,
    roll_id: RollId,
    levels: [u32; 3],
    region: Option<Region>,
) -> CmdResult<JobId> {
    capture::start_calibration(
        &ctx,
        &jobs,
        &hardware,
        Arc::clone(&events.0),
        roll_id,
        levels,
        region,
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
        /// Last, so the folder goes after the app has closed the library database.
        dir: gtp_core::testing::TempDir,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // The mock app keeps its state for ever: release the library before the folder goes.
            crate::testkit::release_context(&self.app);
        }
    }

    fn fixture(label: &str) -> Fixture {
        let app = mock_app();
        crate::ipc::builder::<MockRuntime>().mount_events(&app);
        let dir = gtp_core::testing::TempDir::new(&format!("capture-commands-{label}"));
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
    fn the_camera_settings_commands_read_set_and_refuse_on_the_fakes() {
        let f = fixture("camera-settings");
        let hardware = || f.app.state::<Hardware>();
        let got = camera_settings_get(hardware()).unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].kind, CameraSettingKind::Iso);
        let set = camera_settings_set(hardware(), CameraSettingKind::ShutterSpeed, "1/250".into())
            .unwrap();
        assert_eq!(set[1].value.as_deref(), Some("1/250"));
        let err =
            camera_settings_set(hardware(), CameraSettingKind::Iso, "12345".into()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        // The kind reaches the command as the UI sends it, and the answer has the UI's field names.
        assert_eq!(
            serde_json::from_str::<CameraSettingKind>("\"shutter_speed\"").unwrap(),
            CameraSettingKind::ShutterSpeed
        );
        let json = serde_json::to_value(&set[0]).unwrap();
        assert_eq!(json["kind"], "iso");
        assert!(json["choices"].is_array() && json["writable"].is_boolean());
        let _busy = hardware().busy().acquire().unwrap();
        assert!(camera_settings_get(hardware())
            .unwrap_err()
            .message
            .contains("capture is in progress"));
    }

    #[test]
    fn the_camera_preview_commands_serve_frames_stop_and_refuse_while_busy_on_the_fakes() {
        let f = fixture("camera-preview");
        let hardware = || f.app.state::<Hardware>();
        camera_preview_frame(hardware()).unwrap();
        camera_preview_stop(hardware()).unwrap();
        camera_preview_stop(hardware()).unwrap();
        let _busy = hardware().busy().acquire().unwrap();
        let Err(err) = camera_preview_frame(hardware()) else {
            unreachable!("a frame is refused while the camera is busy");
        };
        assert!(err.message.contains("capture is in progress"), "{err}");
        camera_preview_stop(hardware()).unwrap();
    }

    #[test]
    fn the_camera_zoom_commands_offer_steps_set_zoom_and_area_and_refuse_bad_input_or_busy() {
        let f = fixture("camera-zoom");
        let hardware = || f.app.state::<Hardware>();
        let steps = camera_preview_zoom_steps(hardware()).unwrap();
        let json = serde_json::to_value(&steps).unwrap();
        assert_eq!(json[0]["label"], "Whole frame");
        assert_eq!(
            json[3],
            serde_json::json!({"value": "100%", "label": "100%"})
        );
        camera_preview_set_zoom(hardware(), steps[3].value.clone()).unwrap();
        camera_preview_set_area(hardware(), 0.3, 0.6).unwrap();
        let err = camera_preview_set_zoom(hardware(), "300%".into()).unwrap_err();
        assert!(err.message.contains("not one the camera offers"), "{err}");
        let err = camera_preview_set_area(hardware(), 1.5, 0.5).unwrap_err();
        assert!(err.message.contains("between 0 and 1"), "{err}");
        camera_preview_stop(hardware()).unwrap();
        let _busy = hardware().busy().acquire().unwrap();
        for err in [
            camera_preview_zoom_steps(hardware()).unwrap_err(),
            camera_preview_set_zoom(hardware(), "100%".into()).unwrap_err(),
            camera_preview_set_area(hardware(), 0.5, 0.5).unwrap_err(),
        ] {
            assert!(err.message.contains("capture is in progress"), "{err}");
        }
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
    fn a_focus_check_returns_the_map_and_the_reference_is_saved_and_cleared_through_settings() {
        use crate::commands::system::settings_set;
        use gtp_app::services::focus::FocusPattern;
        use gtp_app::SettingsPatch;

        let f = fixture("focus-check");
        let check = |f: &Fixture| {
            capture_focus_check(
                f.app.state(),
                f.app.state(),
                f.app.state(),
                TripletChannel::Green,
                [44, 22, 2],
            )
            .unwrap()
        };
        let first = check(&f);
        assert_eq!((first.crop_width, first.crop_height), (192, 128));
        assert!(!first.crop.is_empty() && first.sharpness > 0.0);
        assert_eq!(first.pattern, FocusPattern::Bowed);
        assert!(!first.against_reference);

        // Save the raw map as the reference through settings_set: later maps are against it.
        let saved = settings_set(
            f.app.state(),
            SettingsPatch {
                focus_reference: Some(first.raw_map),
                ..SettingsPatch::default()
            },
        )
        .unwrap();
        assert_eq!(saved.focus_reference, Some(first.raw_map));
        let second = check(&f);
        assert!(second.against_reference);
        assert_eq!(second.pattern, FocusPattern::Flat);

        // A bad map is refused and the saved one stays; clearing removes it.
        let err = settings_set(
            f.app.state(),
            SettingsPatch {
                focus_reference: Some([[0.0; 3]; 3]),
                ..SettingsPatch::default()
            },
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let cleared = settings_set(
            f.app.state(),
            SettingsPatch {
                clear_focus_reference: Some(true),
                ..SettingsPatch::default()
            },
        )
        .unwrap();
        assert_eq!(cleared.focus_reference, None);
        assert!(!check(&f).against_reference);
    }

    #[test]
    fn a_focus_check_is_refused_while_the_hardware_is_busy() {
        let f = fixture("focus-busy");
        let _held = f.app.state::<Hardware>().busy().acquire().unwrap();
        let err = capture_focus_check(
            f.app.state(),
            f.app.state(),
            f.app.state(),
            TripletChannel::Green,
            [44, 22, 2],
        )
        .unwrap_err();
        assert!(err.message.contains("capture is in progress"), "{err:?}");
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
    fn a_calibration_returns_a_job_that_ends_with_levels_and_saves_nothing() {
        let f = fixture("calibrate");
        let before = std::fs::read_to_string(f.dir.join("settings.json")).unwrap();
        let rx = listen(&f.app, &["job://done", "job://error", "capture://step"]);
        let id = capture_calibrate(
            f.app.state(),
            f.app.state(),
            f.app.state(),
            f.app.state(),
            f.roll,
            [44, 22, 2],
            None,
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
        assert_eq!(done["result"]["kind"], "Calibrated");
        let levels = done["result"]["levels"].as_array().unwrap();
        assert_eq!(levels.len(), 3);
        assert!(levels[0].as_u64().unwrap() > 44);
        assert!(done["result"]["reached"][0].as_f64().unwrap() > 0.9);
        assert_eq!(done["result"]["warnings"][0]["kind"], "coarse_level");
        assert_eq!(done["result"]["warnings"][0]["channel"], "blue");
        let shots: u64 = done["result"]["shots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap())
            .sum();
        assert_eq!(steps, 3 * shots);
        assert_eq!(
            std::fs::read_to_string(f.dir.join("settings.json")).unwrap(),
            before
        );
    }

    #[test]
    fn a_calibration_area_arrives_as_the_ui_sends_it_and_a_bad_one_is_refused() {
        let region: Region = serde_json::from_str(r#"{"x":0.1,"y":0.2,"w":0.5,"h":0.25}"#).unwrap();
        assert_eq!(
            (region.x, region.y, region.w, region.h),
            (0.1, 0.2, 0.5, 0.25)
        );
        let f = fixture("calibrate-area");
        let err = capture_calibrate(
            f.app.state(),
            f.app.state(),
            f.app.state(),
            f.app.state(),
            f.roll,
            [44, 22, 2],
            Some(Region {
                x: 0.5,
                y: 0.0,
                w: 0.9,
                h: 1.0,
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    // Not with `tether` on macOS: libgphoto2 is the backend there, and this would drive whatever
    // camera is plugged in.
    #[cfg(not(all(target_os = "macos", feature = "tether")))]
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
        let jobs = f.app.state::<JobManager>();
        let err = capture::start_calibration(
            &ctx,
            &jobs,
            &real,
            Arc::clone(&events.0),
            f.roll,
            [44, 22, 2],
            None,
        )
        .unwrap_err();
        assert!(err.message.contains("No camera backend"), "{err}");
        let err = capture::camera_settings(&real).unwrap_err();
        assert!(err.message.contains("No camera backend"), "{err}");
        let err = capture::camera_preview_frame(&real).unwrap_err();
        assert!(err.message.contains("No camera backend"), "{err}");
        let err = capture::camera_preview_stop(&real).unwrap_err();
        assert!(err.message.contains("No camera backend"), "{err}");
        let err = capture::camera_preview_zoom_steps(&real).unwrap_err();
        assert!(err.message.contains("No camera backend"), "{err}");
        let err = capture::camera_preview_set_zoom(&real, "100%").unwrap_err();
        assert!(err.message.contains("No camera backend"), "{err}");
        let err = capture::camera_preview_set_area(&real, 0.5, 0.5).unwrap_err();
        assert!(err.message.contains("No camera backend"), "{err}");
    }
}
