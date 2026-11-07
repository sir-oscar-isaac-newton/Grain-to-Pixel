//! `TauriJobSink`: delivers job events from `gtp-app` to the UI as `job://*` events, and library
//! changes as `library://changed`.

use std::sync::Arc;

use gtp_app::{CaptureEvents, JobSink, LibraryEvents, LightReading, StepState};
use gtp_core::{AppError, ErrorCode, FrameId, JobId, JobResult, RollId, TripletChannel};
use tauri::{AppHandle, Runtime};
use tauri_specta::Event;

use crate::events::{CaptureStep, JobDone, JobError, JobProgress, LibraryChanged, LightStatus};

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

impl<R: Runtime> LibraryEvents for TauriJobSink<R> {
    fn library_changed(&self, roll_id: Option<RollId>) {
        delivered(
            "library://changed",
            LibraryChanged {
                roll_id,
                frame_ids: Vec::new(),
            }
            .emit(&self.app),
        );
    }

    fn frames_changed(&self, frames: &[FrameId]) {
        delivered(
            "library://changed",
            LibraryChanged {
                roll_id: None,
                frame_ids: frames.to_vec(),
            }
            .emit(&self.app),
        );
    }
}

impl<R: Runtime> CaptureEvents for TauriJobSink<R> {
    fn step(&self, channel: TripletChannel, state: StepState) {
        delivered(
            "capture://step",
            CaptureStep { channel, state }.emit(&self.app),
        );
    }

    fn light_status(&self, reading: &LightReading) {
        let status = LightStatus {
            temp_c: reading.temp_c,
            vbus_mv: reading.vbus_mv,
            tripped: reading.tripped,
        };
        delivered("light://status", status.emit(&self.app));
    }
}

/// The library-change channel, managed as app state for commands that start jobs.
pub struct EventsState(pub Arc<dyn LibraryEvents>);

/// The capture-event channel, managed as app state for the capture commands.
pub struct CaptureEventsState(pub Arc<dyn CaptureEvents>);

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
    fn capture_steps_and_the_light_status_reach_the_ui_in_order() {
        let app = mock_app();
        let (tx, rx) = mpsc::channel::<(String, serde_json::Value)>();
        for name in ["capture://step", "light://status"] {
            let tx = tx.clone();
            app.listen_any(name, move |event| {
                let payload = serde_json::from_str(event.payload()).unwrap();
                let _ = tx.send((name.to_owned(), payload));
            });
        }
        let sink = TauriJobSink::new(app.handle().clone());
        sink.step(TripletChannel::Red, StepState::Lit);
        sink.light_status(&LightReading {
            temp_c: Some(31.5),
            vbus_mv: Some(5120),
            tripped: false,
        });
        sink.step(TripletChannel::Red, StepState::Fired);
        sink.light_status(&LightReading {
            temp_c: None,
            vbus_mv: None,
            tripped: true,
        });
        let wait = Duration::from_secs(10);
        let (name, step) = rx.recv_timeout(wait).unwrap();
        assert_eq!(name, "capture://step");
        assert_eq!(
            (step["channel"].as_str(), step["state"].as_str()),
            (Some("red"), Some("lit"))
        );
        let (name, status) = rx.recv_timeout(wait).unwrap();
        assert_eq!(name, "light://status");
        assert_eq!(status["temp_c"], 31.5);
        assert_eq!(status["vbus_mv"], 5120);
        assert_eq!(status["tripped"], false);
        let (_, step) = rx.recv_timeout(wait).unwrap();
        assert_eq!(step["state"], "fired");
        let (_, status) = rx.recv_timeout(wait).unwrap();
        assert_eq!(status["tripped"], true);
        assert!(status["temp_c"].is_null());
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
