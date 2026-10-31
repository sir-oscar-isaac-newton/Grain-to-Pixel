//! System commands.

use gtp_app::JobManager;
use gtp_core::{AppError, ErrorCode, JobId};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::crash::CrashNote;
use crate::error::CmdResult;
use crate::state::AppState;

/// Basic information about the running application.
#[derive(Debug, Serialize, Type, PartialEq, Eq)]
pub struct LibraryInfo {
    pub app_version: String,
}

/// Stub for the real `library_info` of issue L-03; today it only reports the app version.
#[tauri::command]
#[specta::specta]
pub fn library_info() -> CmdResult<LibraryInfo> {
    Ok(LibraryInfo {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

/// Severity of an event reported by the UI.
#[derive(Debug, Clone, Copy, Deserialize, Type, PartialEq, Eq)]
pub enum ClientLogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

const MAX_CLIENT_MESSAGE_CHARS: usize = 2000;

fn check_client_message(message: &str) -> CmdResult<&str> {
    let message = message.trim();
    if message.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The event message is empty.",
        ));
    }
    if message.chars().count() > MAX_CLIENT_MESSAGE_CHARS {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The event message is too long.",
        ));
    }
    Ok(message)
}

fn record_client_event(level: ClientLogLevel, message: &str) {
    match level {
        ClientLogLevel::Error => tracing::error!(target: "client", "{message}"),
        ClientLogLevel::Warn => tracing::warn!(target: "client", "{message}"),
        ClientLogLevel::Info => tracing::info!(target: "client", "{message}"),
        ClientLogLevel::Debug => tracing::debug!(target: "client", "{message}"),
    }
}

/// Writes an event from the UI (error boundary, `window.onerror`) to the application log.
#[tauri::command]
#[specta::specta]
pub fn log_client_event(level: ClientLogLevel, message: String) -> CmdResult<()> {
    record_client_event(level, check_client_message(&message)?);
    Ok(())
}

fn drain_crash_notes(state: &AppState) -> CmdResult<Vec<CrashNote>> {
    let mut notes = state
        .crash_notes
        .lock()
        .map_err(|_| AppError::internal("crash note state is poisoned"))?;
    Ok(std::mem::take(&mut *notes))
}

/// Crashes of earlier runs, reported once so the UI can say something went wrong last time.
#[tauri::command]
#[specta::specta]
pub fn crash_notes_take(state: State<'_, AppState>) -> CmdResult<Vec<CrashNote>> {
    drain_crash_notes(&state)
}

/// Asks a job to stop. It ends with a `job://error` event with code `Cancelled` (or `job://done`
/// if it was already finishing). Returns false when the job is not running, which is harmless.
#[tauri::command]
#[specta::specta]
pub fn job_cancel(jobs: State<'_, JobManager>, id: JobId) -> CmdResult<bool> {
    Ok(jobs.cancel(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_crate_version() {
        assert_eq!(
            library_info().unwrap().app_version,
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn serializes_with_snake_case_field() {
        let json = serde_json::to_string(&library_info().unwrap()).unwrap();
        assert!(json.contains("\"app_version\""));
    }

    #[test]
    fn client_messages_must_be_non_empty_and_bounded() {
        assert_eq!(check_client_message("  boom \n").unwrap(), "boom");
        assert_eq!(
            check_client_message("   ").unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let long = "x".repeat(MAX_CLIENT_MESSAGE_CHARS + 1);
        assert_eq!(
            check_client_message(&long).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn a_client_error_reaches_the_log_with_level_and_message() {
        let buffer = SharedBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(buffer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            log_client_event(ClientLogLevel::Error, "render failed".into()).unwrap();
            log_client_event(ClientLogLevel::Warn, "slow".into()).unwrap();
        });

        let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines[0]["level"], "ERROR");
        assert_eq!(lines[0]["target"], "client");
        assert_eq!(lines[0]["fields"]["message"], "render failed");
        assert_eq!(lines[1]["level"], "WARN");
    }

    #[test]
    fn crash_notes_are_handed_out_once() {
        let state = AppState::default();
        state.crash_notes.lock().unwrap().push(CrashNote {
            file: "crash-1-2.txt".into(),
        });
        assert_eq!(drain_crash_notes(&state).unwrap().len(), 1);
        assert!(drain_crash_notes(&state).unwrap().is_empty());
    }

    #[derive(Clone, Default)]
    struct SharedBuffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedBuffer {
        type Writer = SharedBuffer;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }
}
