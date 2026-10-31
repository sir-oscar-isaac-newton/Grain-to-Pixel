//! Export commands: writing edited frames to files as a background job.

use gtp_app::services::export::{export_frames, ExportOptions, MAX_FRAMES};
use gtp_app::{AppContext, JobKind, JobManager};
use gtp_core::{AppError, ErrorCode, FrameId, JobId, JobResult};
use tauri::State;

use crate::error::CmdResult;

/// Starts exporting the frames at full resolution, each with its saved recipe (the automatic one
/// when it was never edited), as a job; progress and the result (the files written and the frames
/// that failed) arrive as `job://*` events. An existing file is never overwritten: the name gets a
/// number. Returns the job id at once.
#[tauri::command(async)]
#[specta::specta]
pub fn export_start(
    ctx: State<'_, AppContext>,
    jobs: State<'_, JobManager>,
    frame_ids: Vec<FrameId>,
    options: ExportOptions,
) -> CmdResult<JobId> {
    if frame_ids.is_empty() || frame_ids.len() > MAX_FRAMES {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Choose between 1 and 10,000 frames to export.",
        )
        .with_details(format!("{} frames", frame_ids.len())));
    }
    let library = ctx.library()?;
    // Fail now, not inside the job, on options that cannot work.
    options.validate()?;

    let count = frame_ids.len();
    let id = jobs.spawn(JobKind::Export, move |job| {
        let result = export_frames(&library, &frame_ids, &options, job)?;
        tracing::info!(
            exported = result.files.len(),
            failed = result.failed.len(),
            frame_ms = ?result.frame_ms,
            "export finished"
        );
        Ok(JobResult::Exported(result))
    })?;
    tracing::info!(job = %id, frames = count, "export started");
    Ok(id)
}

#[cfg(test)]
mod tests {
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    use gtp_app::services::export::ExportFormat;
    use gtp_core::recipe::OutputSpace;
    use tauri::{Listener, Manager};

    use super::*;
    use crate::sink::TauriJobSink;

    #[test]
    fn an_export_job_checks_its_input_runs_and_reports_through_job_done() {
        let dir = std::env::temp_dir().join(format!(
            "gtp-cmd-export-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let app = tauri::test::mock_app();
        crate::ipc::builder::<tauri::test::MockRuntime>().mount_events(&app);
        let sink = Arc::new(TauriJobSink::new(app.handle().clone()));
        app.manage(JobManager::new(sink.clone()));
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        app.manage(ctx);
        let options = ExportOptions {
            format: ExportFormat::Jpeg,
            space: OutputSpace::Srgb,
            jpeg_quality: 90,
            dest_dir: dir.join("out").to_string_lossy().into_owned(),
            template: "{roll}_{index:03}".into(),
        };
        let start = |ids: Vec<FrameId>, options: ExportOptions| {
            export_start(app.state(), app.state(), ids, options)
        };

        let err = start(vec![FrameId::new()], options.clone()).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "no library is open");

        app.state::<AppContext>()
            .create_library(&dir.join("lib"), "Cmd")
            .unwrap();
        assert_eq!(
            start(vec![], options.clone()).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        for bad in [
            ExportOptions {
                jpeg_quality: 0,
                ..options.clone()
            },
            ExportOptions {
                template: "../x".into(),
                ..options.clone()
            },
            ExportOptions {
                dest_dir: String::new(),
                ..options.clone()
            },
        ] {
            let err = start(vec![FrameId::new()], bad).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
        }

        // A frame that does not exist is a failure in the result, not a failed job.
        let (tx, rx) = mpsc::channel::<serde_json::Value>();
        app.listen_any("job://done", move |event| {
            let _ = tx.send(serde_json::from_str(event.payload()).unwrap());
        });
        let missing = FrameId::new();
        let id = start(vec![missing], options).unwrap();
        let done = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        assert_eq!(done["id"], id.to_string());
        assert_eq!(done["result"]["kind"], "Exported");
        assert_eq!(done["result"]["failed"][0]["frame_id"], missing.to_string());
        assert_eq!(done["result"]["files"].as_array().unwrap().len(), 0);

        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
