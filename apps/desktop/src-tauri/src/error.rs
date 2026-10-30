//! Error mapping for commands. Libraries return their own typed errors; each command converts
//! them to [`gtp_core::AppError`] at this boundary, so the UI always receives `{ code, message, details? }`.

use gtp_core::AppError;

/// The result type of every command.
pub type CmdResult<T> = Result<T, AppError>;
