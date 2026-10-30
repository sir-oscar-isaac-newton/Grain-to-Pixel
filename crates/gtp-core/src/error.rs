//! The error type every command returns to the UI (docs/specification.md section 6).
//!
//! Libraries use their own `thiserror` enums; the adapter maps them to [`AppError`] once.
//! `message` is user-readable and actionable; `details` carries technical context.

use std::fmt;

use serde::Serialize;

/// Stable, machine-readable error category. The UI shows `message` and logs the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ErrorCode {
    NotFound,
    InvalidInput,
    Io,
    Db,
    UnsupportedFormat,
    ScannerUnavailable,
    ScanFailed,
    Cancelled,
    LibraryTooNew,
    Internal,
}

impl ErrorCode {
    /// Every code, in declaration order (used by tests and documentation).
    pub const ALL: [ErrorCode; 10] = [
        ErrorCode::NotFound,
        ErrorCode::InvalidInput,
        ErrorCode::Io,
        ErrorCode::Db,
        ErrorCode::UnsupportedFormat,
        ErrorCode::ScannerUnavailable,
        ErrorCode::ScanFailed,
        ErrorCode::Cancelled,
        ErrorCode::LibraryTooNew,
        ErrorCode::Internal,
    ];
}

/// Error returned to the UI: `{ code, message, details? }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    /// Adds technical context that is logged but not shown as the main message.
    #[must_use]
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn internal(details: impl fmt::Display) -> Self {
        Self::new(
            ErrorCode::Internal,
            "Something went wrong inside the app. Export diagnostics from Settings if it keeps happening.",
        )
        .with_details(details.to_string())
    }

    /// Maps a file-system error: a missing file is `NotFound`, everything else is `Io`.
    pub fn from_io(err: &std::io::Error) -> Self {
        let code = if err.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::NotFound
        } else {
            ErrorCode::Io
        };
        Self::new(code, "A file could not be read or written.").with_details(err.to_string())
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_code_serializes_as_its_name_in_the_error_shape() {
        for code in ErrorCode::ALL {
            let value = serde_json::to_value(AppError::new(code, "msg")).unwrap();
            assert_eq!(
                value,
                json!({ "code": format!("{code:?}"), "message": "msg" })
            );
        }
    }

    #[test]
    fn all_lists_each_code_exactly_once() {
        let mut names: Vec<String> = ErrorCode::ALL.iter().map(|c| format!("{c:?}")).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 10);
    }

    #[test]
    fn details_are_included_only_when_present() {
        let with =
            serde_json::to_value(AppError::new(ErrorCode::Db, "m").with_details("d")).unwrap();
        assert_eq!(with["details"], "d");
        let without = serde_json::to_value(AppError::new(ErrorCode::Db, "m")).unwrap();
        assert!(without.get("details").is_none());
    }

    #[test]
    fn missing_file_maps_to_not_found_and_other_io_to_io() {
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(AppError::from_io(&missing).code, ErrorCode::NotFound);
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(AppError::from_io(&denied).code, ErrorCode::Io);
    }

    #[test]
    fn internal_hides_technical_text_in_details() {
        let err = AppError::internal("boom at line 3");
        assert_eq!(err.code, ErrorCode::Internal);
        assert!(!err.message.contains("boom"));
        assert_eq!(err.details.as_deref(), Some("boom at line 3"));
    }
}
