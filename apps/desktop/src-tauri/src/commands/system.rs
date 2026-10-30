//! System commands.

use serde::Serialize;

/// Basic information about the running application.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct LibraryInfo {
    pub app_version: String,
}

/// Stub for the real `library_info` of issue L-03; today it only reports the app version.
#[tauri::command]
pub fn library_info() -> LibraryInfo {
    LibraryInfo {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_crate_version() {
        assert_eq!(library_info().app_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn serializes_with_snake_case_field() {
        let json = serde_json::to_string(&library_info()).unwrap();
        assert!(json.contains("\"app_version\""));
    }
}
