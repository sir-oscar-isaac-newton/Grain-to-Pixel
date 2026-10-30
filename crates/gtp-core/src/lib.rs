//! Shared domain types: Frame, Roll, Project, Recipe (edit parameters), errors.

pub mod error;

pub use error::{AppError, ErrorCode};

/// Deliberate lint violation for the CI test (issue T-03). Never merged.
pub fn ci_check_deliberate_error() -> u8 {
    let value: Option<u8> = None;
    value.unwrap()
}
