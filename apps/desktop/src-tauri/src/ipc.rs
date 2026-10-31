//! The typed IPC surface: every command and event registered here ends up in the generated
//! `packages/ipc-types/src/bindings.ts`. Run `pnpm gen:ipc` after changing it.

use std::path::Path;

use specta_typescript::Typescript;
use tauri_specta::{collect_commands, collect_events, Builder};

use crate::commands;
use crate::events::{JobDone, JobError, JobProgress};

/// Collects all commands and events (one place, shared by the app, the generator and tests on
/// the mock runtime).
pub fn builder<R: tauri::Runtime>() -> Builder<R> {
    Builder::<R>::new()
        .commands(collect_commands![
            commands::library::library_info,
            commands::library::library_create,
            commands::library::library_open,
            commands::system::settings_get,
            commands::system::settings_set,
            commands::system::log_client_event,
            commands::system::crash_notes_take,
            commands::system::job_cancel
        ])
        .events(collect_events![JobProgress, JobDone, JobError])
}

/// Writes the TypeScript bindings to `path` without starting the app.
pub fn export_bindings(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    builder::<tauri::Wry>().export(Typescript::default(), path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_describe_commands_and_the_error_type() {
        let path = std::env::temp_dir().join(format!("gtp-bindings-{}.ts", std::process::id()));
        export_bindings(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert!(
            text.contains("libraryInfo"),
            "command missing from bindings"
        );
        for command in [
            "logClientEvent",
            "crashNotesTake",
            "jobCancel",
            "libraryCreate",
            "libraryOpen",
            "settingsGet",
            "settingsSet",
        ] {
            assert!(text.contains(command), "{command} missing from bindings");
        }
        for event in ["job://progress", "job://done", "job://error"] {
            assert!(text.contains(event), "event {event} missing from bindings");
        }
        for ty in ["JobProgress", "JobDone", "JobError", "JobResult"] {
            assert!(text.contains(ty), "type {ty} missing from bindings");
        }
        assert!(text.contains("AppError"), "AppError missing from bindings");
        for code in gtp_core::ErrorCode::ALL {
            assert!(
                text.contains(&format!("{code:?}")),
                "ErrorCode::{code:?} missing"
            );
        }
    }

    #[test]
    fn export_is_deterministic() {
        let dir = std::env::temp_dir();
        let a = dir.join(format!("gtp-bindings-a-{}.ts", std::process::id()));
        let b = dir.join(format!("gtp-bindings-b-{}.ts", std::process::id()));
        export_bindings(&a).unwrap();
        export_bindings(&b).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
        std::fs::remove_file(&a).unwrap();
        std::fs::remove_file(&b).unwrap();
    }
}
