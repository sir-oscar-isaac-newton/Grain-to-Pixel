//! Tauri adapter: registers commands, events and app state. It validates input, calls
//! application services and maps errors; it holds no business logic (docs/architecture.md section 3).

mod commands;
mod error;
mod events;
mod ipc;
mod state;

use std::path::{Path, PathBuf};

use tauri::webview::PageLoadEvent;

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
            Ok(())
        })
        .run(tauri::generate_context!())
}

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
