//! Library commands: create, open and describe the library. They run off the main thread, since
//! opening may migrate the database or run its integrity check.

use std::path::PathBuf;

use gtp_app::{AppContext, LibraryDetails};
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_reports_the_crate_version_and_no_library_by_default() {
        let i = info(None);
        assert_eq!(i.app_version, env!("CARGO_PKG_VERSION"));
        let json = serde_json::to_value(&i).unwrap();
        assert_eq!(json["library"], serde_json::Value::Null);
        assert!(json.get("app_version").is_some());
    }
}
