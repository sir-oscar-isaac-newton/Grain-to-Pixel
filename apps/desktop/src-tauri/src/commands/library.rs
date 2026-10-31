//! Library commands: create, open and describe the library. They run off the main thread, since
//! opening may migrate the database or run its integrity check.

use std::path::PathBuf;

use gtp_app::services::library::{check_folder, FolderCheck};
use gtp_app::services::verify::verify_library;
use gtp_app::{AppContext, JobKind, JobManager, LibraryDetails};
use gtp_core::{JobId, JobResult};
use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::error::CmdResult;

/// The running app and the open library, if any.
#[derive(Debug, Serialize, Type, PartialEq)]
pub struct LibraryInfo {
    pub app_version: String,
    /// `null` until a library is created or opened.
    pub library: Option<LibraryDetails>,
}

fn info(library: Option<LibraryDetails>) -> LibraryInfo {
    LibraryInfo {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        library,
    }
}

/// The app version and the open library.
#[tauri::command(async)]
#[specta::specta]
pub fn library_info(ctx: State<'_, AppContext>) -> CmdResult<LibraryInfo> {
    Ok(info(ctx.library_details()?))
}

/// Creates a library in `path` (missing or an empty folder) and opens it.
#[tauri::command(async)]
#[specta::specta]
pub fn library_create(
    ctx: State<'_, AppContext>,
    path: String,
    name: String,
) -> CmdResult<LibraryInfo> {
    let details = ctx.create_library(&PathBuf::from(path), &name)?;
    tracing::info!(path = %details.path, "library created");
    Ok(info(Some(details)))
}

/// Opens the library in `path`. A library from a newer app version opens read-only.
#[tauri::command(async)]
#[specta::specta]
pub fn library_open(ctx: State<'_, AppContext>, path: String) -> CmdResult<LibraryInfo> {
    let details = ctx.open_library(&PathBuf::from(path))?;
    tracing::info!(path = %details.path, read_only = details.read_only, "library opened");
    Ok(info(Some(details)))
}

/// Describes a folder the user picked (does it exist, is it empty, does it hold a library, how
/// much space is free) for the first-run dialog. Nothing is created.
#[tauri::command(async)]
#[specta::specta]
pub fn folder_check(path: String) -> CmdResult<FolderCheck> {
    check_folder(&PathBuf::from(path))
}

/// Starts verifying every original of the open library as a job (see `services::verify`). It only
/// reads: progress and the `VerifyResult` arrive as `job://*` events, and it can be cancelled with
/// `job_cancel`. Works on a read-only library too.
#[tauri::command(async)]
#[specta::specta]
pub fn library_verify(ctx: State<'_, AppContext>, jobs: State<'_, JobManager>) -> CmdResult<JobId> {
    let library = ctx.library()?;
    let id = jobs.spawn(JobKind::LibraryVerify, move |job| {
        let result = verify_library(&library, job)?;
        tracing::info!(
            files = result.files_checked,
            problems = result.problems.len(),
            thumbnails_missing = result.thumbnails_missing,
            stray_folders = result.stray_folders.len(),
            "library verify finished"
        );
        Ok(JobResult::Verified(result))
    })?;
    tracing::info!(job = %id, "library verify started");
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verify_job_runs_and_reports_through_job_done() {
        use std::sync::{mpsc, Arc};
        use std::time::Duration;

        use gtp_core::{ErrorCode, FrameId};
        use tauri::{Listener, Manager};

        let dir = std::env::temp_dir().join(format!(
            "gtp-cmd-verify-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let app = tauri::test::mock_app();
        crate::ipc::builder::<tauri::test::MockRuntime>().mount_events(&app);
        app.manage(JobManager::new(Arc::new(crate::sink::TauriJobSink::new(
            app.handle().clone(),
        ))));
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        app.manage(ctx);

        let err = library_verify(app.state(), app.state()).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "no library is open");

        app.state::<AppContext>()
            .create_library(&dir.join("lib"), "Cmd")
            .unwrap();
        let (tx, rx) = mpsc::channel::<serde_json::Value>();
        app.listen_any("job://done", move |event| {
            let _ = tx.send(serde_json::from_str(event.payload()).unwrap());
        });
        let id = library_verify(app.state(), app.state()).unwrap();
        let done = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        assert_eq!(done["id"], id.to_string());
        assert_eq!(done["result"]["kind"], "Verified");
        assert_eq!(done["result"]["files_checked"], 0);
        assert_eq!(done["result"]["problems"], serde_json::json!([]));

        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn info_reports_the_crate_version_and_no_library_by_default() {
        let i = info(None);
        assert_eq!(i.app_version, env!("CARGO_PKG_VERSION"));
        let json = serde_json::to_value(&i).unwrap();
        assert_eq!(json["library"], serde_json::Value::Null);
        assert!(json.get("app_version").is_some());
    }
}
