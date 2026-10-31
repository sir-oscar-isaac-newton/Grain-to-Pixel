//! Tauri adapter: registers commands, events and app state. It validates input, calls
//! application services and maps errors; it holds no business logic (docs/architecture.md section 3).

mod commands;
mod crash;
mod error;
mod events;
mod ipc;
mod logging;
mod sink;
mod state;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gtp_app::{AppContext, JobManager};
use tauri::webview::PageLoadEvent;
use tauri::{Manager, RunEvent};

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
    let builder = ipc::builder::<tauri::Wry>();
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
            let sink = sink::TauriJobSink::new(app.handle().clone());
            app.manage(JobManager::new(Arc::new(sink)));
            app.manage(app_context(app));
            Ok(())
        })
        .build(tauri::generate_context!())?
        .run(|app, event| {
            if let RunEvent::Exit = event {
                // Running jobs stop at their next check instead of being cut off mid-write.
                if let Some(jobs) = app.try_state::<JobManager>() {
                    jobs.cancel_all();
                }
            }
        });
    Ok(())
}

/// The shared context with `settings.json` from the OS app-config folder. Without one (very
/// unusual) settings live in the temp folder for this run, so the app still starts.
fn app_context(app: &tauri::App) -> AppContext {
    let dir = app.path().app_config_dir().unwrap_or_else(|err| {
        tracing::warn!(%err, "no app config folder; settings are kept in the temp folder");
        std::env::temp_dir().join("grain-to-pixel")
    });
    let (ctx, note) = AppContext::new(&dir.join("settings.json"));
    if let Some(note) = note {
        tracing::warn!(%note, "settings reset to defaults");
    }
    ctx
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
