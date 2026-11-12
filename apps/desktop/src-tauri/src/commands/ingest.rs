//! Ingest commands: importing files as a background job, and grouping camera files into triplets.

use std::path::PathBuf;
use std::sync::Arc;

use gtp_app::services::import::{import_tiffs_with_look, ImportKind};
use gtp_app::services::import_triplets::import_triplets_with_look;
use gtp_app::services::roll_frame;
use gtp_app::services::scan::{self, ScanRequest};
use gtp_app::services::scan_preview::{RollPreviewImage, ScanPreviewService};
use gtp_app::services::triplets;
use gtp_app::{
    AppContext, DeviceInfo, Hardware, JobKind, JobManager, RawlerDecoder, ScannerSource,
    TripletCandidate,
};
use gtp_core::{AppError, ErrorCode, JobId, JobResult, RollId};
use tauri::State;

use crate::error::CmdResult;
use crate::sink::EventsState;

/// Most files one import may name.
const MAX_FILES: usize = 10_000;

/// Starts importing `paths` into the roll as a job; progress and the result arrive as `job://*`
/// events, each new frame as `library://changed`. Returns the job id at once.
#[tauri::command(async)]
#[specta::specta]
pub fn import_files(
    ctx: State<'_, AppContext>,
    jobs: State<'_, JobManager>,
    events: State<'_, EventsState>,
    paths: Vec<String>,
    roll_id: RollId,
    kind: ImportKind,
) -> CmdResult<JobId> {
    if kind == ImportKind::RgbTriplet {
        if !ctx.settings().get().flags.triplet_import {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "Importing RGB triplets is switched off in Settings.",
            ));
        }
        if !paths.len().is_multiple_of(3) {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "A triplet is three files: the red-, green- and blue-lit exposure.",
            )
            .with_details(format!("{} files", paths.len())));
        }
    }
    if paths.is_empty() || paths.len() > MAX_FILES {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Choose between 1 and 10,000 files to import.",
        )
        .with_details(format!("{} files", paths.len())));
    }
    let library = ctx.library()?;
    // Fail now, not inside the job, when the roll is gone or the library is read-only.
    library.read(|c| c.get_roll(roll_id))?;
    library.write(|_| Ok(()))?;

    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let events = Arc::clone(&events.0);
    let count = paths.len();
    let thumbnails = ctx.thumbnail_requester()?;
    let look = ctx.settings().get().starting_look;
    let id = jobs.spawn(JobKind::Import, move |job| {
        let result = match kind {
            ImportKind::TiffScan => {
                import_tiffs_with_look(&library, roll_id, &paths, job, events.as_ref(), look)?
            }
            ImportKind::RgbTriplet => import_triplets_with_look(
                &library,
                roll_id,
                &paths,
                &RawlerDecoder,
                job,
                events.as_ref(),
                look,
            )?,
        };
        // A better film base for the roll changes how its other frames look: refresh their thumbnails.
        if result.base_replaced {
            let following = roll_frame::frames_following_roll_base(&library, roll_id)?;
            thumbnails.request(Arc::clone(&events), following);
        }
        // The roll's colour balance was written into these frames' recipes when the job ended.
        thumbnails.request(Arc::clone(&events), result.rebalanced.clone());
        tracing::info!(
            imported = result.frames.len(),
            skipped = result.skipped.len(),
            warnings = result.warnings.len(),
            frame_ms = ?result.frame_ms,
            "import finished"
        );
        Ok(JobResult::Imported(result))
    })?;
    tracing::info!(job = %id, roll = %roll_id, files = count, "import started");
    Ok(id)
}

/// Groups camera files (NEF) into R/G/B triplet candidates by capture time: each candidate lists
/// its files red first, then green, then blue, and says when a shot is missing or extra. Reads the
/// files' EXIF only and changes nothing.
#[tauri::command(async)]
#[specta::specta]
pub fn triplet_group(paths: Vec<String>) -> CmdResult<Vec<TripletCandidate>> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    triplets::triplet_group(&paths)
}

/// The scanner of the app (nkscan, or the fixture one with `GTP_FAKE_HW=1`).
fn scanner(hardware: &Hardware) -> Result<Arc<dyn ScannerSource>, AppError> {
    hardware
        .scanner
        .clone()
        .ok_or_else(|| AppError::new(ErrorCode::ScannerUnavailable, "No scanner is set up."))
}

/// The scanners that can be used right now; `ScannerUnavailable` (with the reason in its message:
/// not connected, wrong driver, nkscan missing) when there is none.
#[tauri::command(async)]
#[specta::specta]
pub fn scanner_list(hardware: State<'_, Hardware>) -> CmdResult<Vec<DeviceInfo>> {
    scanner(&hardware)?.list_devices()
}

/// The frame number a scan of the roll should resume at: one after the last frame scanned into it,
/// or 1 when none was (above 40 the roll is done).
#[tauri::command(async)]
#[specta::specta]
pub fn scan_resume_point(ctx: State<'_, AppContext>, roll_id: RollId) -> CmdResult<u32> {
    scan::scan_resume_point(&*ctx.library()?, roll_id)
}

/// Starts scanning the requested frames into the roll as a job. Each frame is stored as soon as the
/// scanner finishes it, so a cancel or an error keeps the frames before it; progress and the
/// result (frames stored, like an import's) arrive as `job://*` events, each new frame as
/// `library://changed`. One scan, and no roll preview, at a time. Returns the job id at once.
#[tauri::command(async)]
#[specta::specta]
pub fn scan_start(
    ctx: State<'_, AppContext>,
    jobs: State<'_, JobManager>,
    events: State<'_, EventsState>,
    hardware: State<'_, Hardware>,
    preview: State<'_, ScanPreviewService>,
    roll_id: RollId,
    request: ScanRequest,
) -> CmdResult<JobId> {
    let params = request.params()?;
    let scanner = scanner(&hardware)?;
    let library = ctx.library()?;
    // Fail now, not inside the job, when the roll is gone or the library is read-only.
    library.read(|c| c.get_roll(roll_id))?;
    library.write(|_| Ok(()))?;
    if jobs
        .running()
        .iter()
        .any(|(_, kind)| *kind == JobKind::Scan)
        || preview.is_running()
    {
        return Err(AppError::new(
            ErrorCode::ScannerUnavailable,
            "The scanner is busy with another scan or a roll preview.",
        ));
    }

    let events = Arc::clone(&events.0);
    let thumbnails = ctx.thumbnail_requester()?;
    let look = ctx.settings().get().starting_look;
    let (first, last) = (
        params.frames.start().to_owned(),
        params.frames.end().to_owned(),
    );
    let id = jobs.spawn(JobKind::Scan, move |job| {
        let result = scan::scan_roll(
            &library,
            roll_id,
            scanner.as_ref(),
            &params,
            job,
            events.as_ref(),
            look,
        )?;
        // A better film base for the roll changes how its other frames look: refresh their thumbnails.
        if result.base_replaced {
            let following = roll_frame::frames_following_roll_base(&library, roll_id)?;
            thumbnails.request(Arc::clone(&events), following);
        }
        // The roll's colour balance was written into these frames' recipes when the job ended.
        thumbnails.request(Arc::clone(&events), result.rebalanced.clone());
        tracing::info!(
            stored = result.frames.len(),
            frame_ms = ?result.frame_ms,
            "scan finished"
        );
        Ok(JobResult::Imported(result))
    })?;
    tracing::info!(job = %id, roll = %roll_id, first, last, "scan started");
    Ok(id)
}

/// The whole roll as the scanner's framing thumbnail, made into a positive strip, with the number of
/// frames the scanner found and the pitch they are evenly spaced at (strip pixels). Scans no frame
/// and leaves the film in; it takes about a minute on the LS-50. One at a time; see
/// `scan_preview_cancel`.
#[tauri::command(async)]
#[specta::specta]
pub fn scan_preview(
    hardware: State<'_, Hardware>,
    preview: State<'_, ScanPreviewService>,
    jobs: State<'_, JobManager>,
    device_id: String,
) -> CmdResult<RollPreviewImage> {
    if jobs
        .running()
        .iter()
        .any(|(_, kind)| *kind == JobKind::Scan)
    {
        return Err(AppError::new(
            ErrorCode::ScannerUnavailable,
            "The scanner is busy with a scan.",
        ));
    }
    let scanner = scanner(&hardware)?;
    let image = preview.run(scanner.as_ref(), &device_id)?;
    tracing::info!(
        frames = image.frame_count,
        width = image.strip_width,
        "roll previewed"
    );
    Ok(image)
}

/// Stops the running roll preview; false when none is running.
#[tauri::command(async)]
#[specta::specta]
pub fn scan_preview_cancel(preview: State<'_, ScanPreviewService>) -> bool {
    preview.cancel()
}

#[cfg(test)]
mod tests {
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    use gtp_app::services::roll_frame::{roll_create, RollInput};
    use tauri::{Listener, Manager};

    use super::*;
    use crate::sink::TauriJobSink;

    #[test]
    fn a_preview_needs_a_scanner_and_cancel_with_none_running_says_so() {
        let app = tauri::test::mock_app();
        let temp = gtp_core::testing::TempDir::new("cmd-no-roll");
        let missing = temp.join("none");
        app.manage(Hardware::from_vars(Some("1"), Some(missing.into())));
        app.manage(ScanPreviewService::default());
        app.manage(JobManager::new(Arc::new(TauriJobSink::new(
            app.handle().clone(),
        ))));
        let err =
            scan_preview(app.state(), app.state(), app.state(), "fixture".into()).unwrap_err();
        assert_eq!(err.code, ErrorCode::ScannerUnavailable);
        assert!(!scan_preview_cancel(app.state()));
    }

    #[test]
    fn a_scan_is_refused_without_a_roll_or_valid_parameters_and_a_failing_scan_reports_an_error() {
        let dir = gtp_core::testing::TempDir::new("cmd-scan");
        let empty = dir.join("no-scans");
        std::fs::create_dir_all(&empty).unwrap();
        let app = tauri::test::mock_app();
        crate::ipc::builder::<tauri::test::MockRuntime>().mount_events(&app);
        let sink = Arc::new(TauriJobSink::new(app.handle().clone()));
        app.manage(JobManager::new(sink.clone()));
        app.manage(EventsState(sink));
        app.manage(Hardware::from_vars(Some("1"), Some(empty.clone().into())));
        app.manage(ScanPreviewService::default());
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        app.manage(ctx);
        let _release = crate::testkit::ReleaseContext(&app);
        let request = |first: u32, last: u32| ScanRequest {
            device_id: "fixture".into(),
            first_frame: first,
            last_frame: last,
            dpi: 2000,
            samples: 1,
            ir: true,
            autofocus: false,
        };
        let start = |roll: RollId, request: ScanRequest| {
            scan_start(
                app.state(),
                app.state(),
                app.state(),
                app.state(),
                app.state(),
                roll,
                request,
            )
        };

        let err = start(RollId::new(), request(0, 3)).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput, "frames start at 1");
        let err = start(RollId::new(), request(1, 3)).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "no library is open");

        let ctx = app.state::<AppContext>();
        ctx.create_library(&dir.join("lib"), "Cmd").unwrap();
        let err = start(RollId::new(), request(1, 3)).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "the roll must exist");
        let roll = roll_create(
            &ctx.library().unwrap(),
            RollInput {
                name: "R".into(),
                ..RollInput::default()
            },
        )
        .unwrap();
        assert_eq!(scan_resume_point(app.state(), roll.id).unwrap(), 1);
        assert_eq!(
            scan_resume_point(app.state(), RollId::new())
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        // The fixture folder has no scans, so the scanner fails at frame 1: the job reports the error.
        let (tx, rx) = mpsc::channel::<serde_json::Value>();
        app.listen_any("job://error", move |event| {
            let _ = tx.send(serde_json::from_str(event.payload()).unwrap());
        });
        let id = start(roll.id, request(1, 2)).unwrap();
        let error = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        assert_eq!(error["id"], id.to_string());
        assert_eq!(error["error"]["code"], "ScanFailed");
        assert_eq!(scan_resume_point(app.state(), roll.id).unwrap(), 1);
    }

    #[test]
    fn an_import_job_runs_and_reports_through_job_done() {
        let dir = gtp_core::testing::TempDir::new("cmd-import");
        let app = tauri::test::mock_app();
        crate::ipc::builder::<tauri::test::MockRuntime>().mount_events(&app);
        let sink = Arc::new(TauriJobSink::new(app.handle().clone()));
        app.manage(JobManager::new(sink.clone()));
        app.manage(EventsState(sink));
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        app.manage(ctx);
        let _release = crate::testkit::ReleaseContext(&app);
        let start = |paths: Vec<String>, roll: RollId, kind: ImportKind| {
            import_files(app.state(), app.state(), app.state(), paths, roll, kind)
        };

        let err = start(vec!["a.tif".into()], RollId::new(), ImportKind::TiffScan).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "no library is open");

        let ctx = app.state::<AppContext>();
        ctx.create_library(&dir.join("lib"), "Cmd").unwrap();
        let roll = roll_create(
            &ctx.library().unwrap(),
            RollInput {
                name: "R".into(),
                ..RollInput::default()
            },
        )
        .unwrap();
        let paths = vec![dir.join("notes.txt").to_string_lossy().into_owned()];
        for (paths, roll, kind) in [
            (paths.clone(), roll.id, ImportKind::RgbTriplet),
            (vec![], roll.id, ImportKind::TiffScan),
        ] {
            assert_eq!(
                start(paths, roll, kind).unwrap_err().code,
                ErrorCode::InvalidInput
            );
        }
        let err = start(paths.clone(), RollId::new(), ImportKind::TiffScan).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "the roll must exist");

        let (tx, rx) = mpsc::channel::<serde_json::Value>();
        app.listen_any("job://done", move |event| {
            let _ = tx.send(serde_json::from_str(event.payload()).unwrap());
        });
        let id = start(paths, roll.id, ImportKind::TiffScan).unwrap();
        let done = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        assert_eq!(done["id"], id.to_string());
        assert_eq!(done["result"]["kind"], "Imported");
        assert_eq!(done["result"]["skipped"][0]["reason"], "NotTiff");

        // A triplet import runs through the same job: files that are not camera files are skipped,
        // all three of them, and there is nothing to warn about.
        let nefs: Vec<String> = ["r.NEF", "g.NEF", "b.NEF"]
            .iter()
            .map(|name| {
                let path = dir.join(name);
                std::fs::write(&path, b"not a camera file").unwrap();
                path.to_string_lossy().into_owned()
            })
            .collect();
        let id = start(nefs.clone(), roll.id, ImportKind::RgbTriplet).unwrap();
        let done = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        assert_eq!(done["id"], id.to_string());
        assert_eq!(done["result"]["frames"].as_array().unwrap().len(), 0);
        assert_eq!(done["result"]["skipped"].as_array().unwrap().len(), 3);
        assert_eq!(done["result"]["skipped"][0]["reason"], "Unsupported");
        assert_eq!(done["result"]["warnings"].as_array().unwrap().len(), 0);

        // Switched off in Settings, it is refused.
        ctx.settings()
            .update(&gtp_app::SettingsPatch {
                flags: Some(gtp_app::FlagsPatch {
                    triplet_import: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
        let err = start(nefs, roll.id, ImportKind::RgbTriplet).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(err.message.contains("switched off"), "{}", err.message);
    }

    #[test]
    fn triplet_group_checks_its_input_and_answers_in_the_shape_the_ui_reads() {
        let empty = triplet_group(Vec::new()).unwrap_err();
        assert_eq!(empty.code, ErrorCode::InvalidInput);
        let missing = triplet_group(vec!["no-such-folder/a.NEF".into()]).unwrap_err();
        assert_eq!(missing.code, ErrorCode::NotFound);

        // Files without EXIF cannot be grouped: one flagged candidate, files by name.
        let dir = gtp_core::testing::TempDir::new("triplet-cmd");
        let paths: Vec<String> = ["b.NEF", "a.NEF", "c.NEF"]
            .iter()
            .map(|name| {
                let path = dir.join(name);
                std::fs::write(&path, b"not a camera file").unwrap();
                path.display().to_string()
            })
            .collect();
        let candidates = triplet_group(paths).unwrap();
        let json = serde_json::to_value(&candidates).unwrap();
        assert_eq!(json[0]["problem"]["kind"], "no_capture_time");
        assert_eq!(json[0]["files"].as_array().unwrap().len(), 3);
        assert!(json[0]["files"][0]["path"]
            .as_str()
            .unwrap()
            .ends_with("a.NEF"));
        assert!(json[0]["files"][0]["captured"].is_null());
        assert!(
            json[0]["files"][0]["channel"].is_null(),
            "no preview, so no colour"
        );
    }
}
