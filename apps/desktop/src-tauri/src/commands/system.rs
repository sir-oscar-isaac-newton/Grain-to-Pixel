//! System commands: settings, client logging, crash notes and jobs.

use gtp_app::{AppContext, JobManager, Settings, SettingsPatch};
use gtp_core::{AppError, ErrorCode, JobId};
use serde::Deserialize;
use specta::Type;
use tauri::State;

use crate::crash::CrashNote;
use crate::error::CmdResult;
use crate::state::AppState;

/// The app settings (spec section 5.4), feature flags included.
#[tauri::command]
#[specta::specta]
pub fn settings_get(ctx: State<'_, AppContext>) -> CmdResult<Settings> {
    Ok(ctx.settings().get())
}

/// The settings as they are before anyone changes them, so the UI can say what the default of a
/// feature flag is without carrying a copy of it.
#[tauri::command]
#[specta::specta]
pub fn settings_defaults() -> Settings {
    Settings::default()
}

/// Changes the settings named in `patch`, saves them and returns the result.
#[tauri::command]
#[specta::specta]
pub fn settings_set(ctx: State<'_, AppContext>, patch: SettingsPatch) -> CmdResult<Settings> {
    let settings = ctx.settings().update(&patch)?;
    ctx.develop().set_dust_removal(settings.flags.dust_removal);
    Ok(settings)
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
    fn the_defaults_are_the_flags_of_the_specification_and_nothing_changed() {
        let defaults = settings_defaults();
        let flags = defaults.flags;
        // Spec section 10: these three are on, these two off.
        assert!(flags.dust_removal && flags.triplet_import && flags.guided_capture);
        assert!(!flags.dev_overlay && !flags.experimental_auto_levels);
        assert_eq!(defaults, Settings::default());
        assert_eq!(defaults.last_library, None);
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
