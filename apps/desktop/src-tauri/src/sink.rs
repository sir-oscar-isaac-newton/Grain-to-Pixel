//! `TauriJobSink`: delivers job events from `gtp-app` to the UI as `job://*` events.

use gtp_app::JobSink;
use gtp_core::{AppError, ErrorCode, JobId, JobResult};
use tauri::{AppHandle, Runtime};
use tauri_specta::Event;

use crate::events::{JobDone, JobError, JobProgress};

pub struct TauriJobSink<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriJobSink<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

/// An event that cannot be delivered (the window is gone while quitting) is only logged.
fn delivered(name: &str, result: tauri::Result<()>) {
    if let Err(err) = result {
        tracing::warn!(%err, event = name, "job event not delivered");
    }
}

impl<R: Runtime> JobSink for TauriJobSink<R> {
    fn progress(&self, id: JobId, stage: &str, fraction: f32) {
        let event = JobProgress {
            id,
            stage: stage.to_owned(),
            fraction,
        };
        delivered("job://progress", event.emit(&self.app));
    }

    fn done(&self, id: JobId, result: JobResult) {
        tracing::info!(job = %id, "job done");
        delivered("job://done", JobDone { id, result }.emit(&self.app));
    }

    fn error(&self, id: JobId, error: AppError) {
        if error.code == ErrorCode::Cancelled {
            tracing::info!(job = %id, "job cancelled");
        } else {
            tracing::warn!(job = %id, code = ?error.code, message = %error.message, details = ?error.details, "job failed");
        }
        delivered("job://error", JobError { id, error }.emit(&self.app));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use gtp_app::{JobKind, JobManager};
    use tauri::Listener;

    use super::*;

    /// A mock app with the typed events mounted, as `run` does at setup.
    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        crate::ipc::builder::<tauri::test::MockRuntime>().mount_events(&app);
        app
    }

    #[test]
    fn a_job_reaches_the_ui_as_progress_then_done() {
        let app = mock_app();
        let (tx, rx) = mpsc::channel::<(String, serde_json::Value)>();
        for name in ["job://progress", "job://done", "job://error"] {
            let tx = tx.clone();
            app.listen_any(name, move |event| {
                let payload = serde_json::from_str(event.payload()).unwrap();
                let _ = tx.send((name.to_owned(), payload));
            });
        }
        let jobs = JobManager::new(Arc::new(TauriJobSink::new(app.handle().clone())));
        let id = jobs
            .spawn(JobKind::Export, |ctx| {
                ctx.progress("encoding", 0.5);
                Ok(JobResult::Finished)
            })
            .unwrap();

        let wait = Duration::from_secs(10);
        let (name, progress) = rx.recv_timeout(wait).unwrap();
        assert_eq!(name, "job://progress");
        assert_eq!(progress["id"], id.to_string());
        assert_eq!(progress["stage"], "encoding");
        assert_eq!(progress["fraction"], 0.5);
        let (name, done) = rx.recv_timeout(wait).unwrap();
        assert_eq!(name, "job://done");
        assert_eq!(done["result"]["kind"], "Finished");
    }

    #[test]
    fn a_cancelled_job_reaches_the_ui_as_error_with_code_cancelled() {
        let app = mock_app();
        let (tx, rx) = mpsc::channel::<serde_json::Value>();
        app.listen_any("job://error", move |event| {
            let _ = tx.send(serde_json::from_str(event.payload()).unwrap());
        });
        let jobs = JobManager::new(Arc::new(TauriJobSink::new(app.handle().clone())));
        let id = jobs
            .spawn(JobKind::Scan, |ctx| loop {
                ctx.check_cancelled()?;
                std::thread::sleep(Duration::from_millis(5));
            })
            .unwrap();
        assert!(jobs.cancel(id));
        let error = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(error["id"], id.to_string());
        assert_eq!(error["error"]["code"], "Cancelled");
    }
}
