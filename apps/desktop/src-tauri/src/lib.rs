//! Tauri adapter: registers commands, events and app state. It validates input, calls
//! application services and maps errors; it holds no business logic (docs/architecture.md section 3).

mod commands;
mod crash;
mod error;
mod events;
mod ipc;
mod logging;
mod state;

use std::path::{Path, PathBuf};

use tauri::webview::PageLoadEvent;
use tauri::Manager;

pub use ipc::export_bindings;

/// Environment variable naming a file the app writes once its page has loaded. Used only by the
/// smoke test (tools/scripts/smoke) as a "started and rendered" marker; absent in normal use.
const READY_FILE_ENV: &str = "GTP_READY_FILE";

fn ready_file_path() -> Option<PathBuf> {
    std::env::var_os(READY_FILE_ENV).map(PathBuf::from)
}

fn signal_ready(path: &Path) -> std::io::Result<()> {
    std::fs::write(path, env!("CARGO_PKG_VERSION"))
}

/// Builds and runs the desktop application.
pub fn run() -> tauri::Result<()> {
    let builder = ipc::builder();
    tauri::Builder::default()
        .manage(state::AppState::default())
        .on_page_load(|_webview, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                if let Some(path) = ready_file_path() {
                    // Best effort: a missing marker makes the smoke test fail, which is the signal.
                    let _ = signal_ready(&path);
                }
            }
        })
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            start_observability(app);
            Ok(())
        })
        .run(tauri::generate_context!())
}

/// Starts file logging and the panic hook, then collects crash notes from earlier runs.
/// Failures never stop the app: without a log directory it runs with stderr logging only.
fn start_observability(app: &tauri::App) {
    let Ok(dir) = app.path().app_log_dir() else {
        return;
    };
    match logging::init(&dir) {
        Ok(guard) => {
            // Dropping the guard would stop the log writer, so it lives as long as the app.
            app.manage(LogGuard(guard));
        }
        Err(err) => {
            // Another subscriber (tests, dev tooling) or an unwritable directory: keep going.
            let _ = tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .try_init();
            tracing::warn!(%err, "file logging is off");
        }
    }
    crash::install_hook(dir.clone());
    match crash::take_unreported(&dir) {
        Ok(notes) => {
            if let Ok(mut slot) = app.state::<state::AppState>().crash_notes.lock() {
                *slot = notes;
            }
        }
        Err(err) => tracing::warn!(%err, "could not read crash notes"),
    }
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "grain to pixel started"
    );
}

/// Keeps the non-blocking log writer alive (see [`start_observability`]).
struct LogGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_ready_writes_the_app_version() {
        let path = std::env::temp_dir().join(format!("gtp-ready-{}.txt", std::process::id()));
        signal_ready(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            env!("CARGO_PKG_VERSION")
        );
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn signal_ready_reports_an_unwritable_path() {
        let path = std::env::temp_dir()
            .join("gtp-no-such-dir")
            .join("ready.txt");
        assert!(signal_ready(&path).is_err());
    }
}
