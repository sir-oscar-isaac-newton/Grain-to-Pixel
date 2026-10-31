//! Ingest commands: importing files as a background job.

use std::path::PathBuf;

use gtp_app::services::import::{import_tiffs, ImportKind};
use gtp_app::{AppContext, JobKind, JobManager};
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
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Importing RGB triplets is not available yet.",
        ));
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
    let events = std::sync::Arc::clone(&events.0);
    let count = paths.len();
    let id = jobs.spawn(JobKind::Import, move |job| {
        let result = import_tiffs(&library, roll_id, &paths, job, events.as_ref())?;
        tracing::info!(
            imported = result.frames.len(),
            skipped = result.skipped.len(),
            frame_ms = ?result.frame_ms,
            "import finished"
        );
        Ok(JobResult::Imported(result))
    })?;
    tracing::info!(job = %id, roll = %roll_id, files = count, "import started");
    Ok(id)
}

#[cfg(test)]
mod tests {
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    use gtp_app::services::roll_frame::{roll_create, RollInput};
    use gtp_core::FrameId;
    use tauri::{Listener, Manager};

    use super::*;
    use crate::sink::TauriJobSink;

    #[test]
    fn an_import_job_runs_and_reports_through_job_done() {
        let dir = std::env::temp_dir().join(format!(
            "gtp-cmd-import-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let app = tauri::test::mock_app();
        crate::ipc::builder::<tauri::test::MockRuntime>().mount_events(&app);
        let sink = Arc::new(TauriJobSink::new(app.handle().clone()));
        app.manage(JobManager::new(sink.clone()));
        app.manage(EventsState(sink));
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        app.manage(ctx);
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

        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
