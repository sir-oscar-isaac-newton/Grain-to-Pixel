//! Repositories: one file per entity, each adding methods to [`Catalog`](crate::Catalog). Reads take
//! `&self`; writes take `&mut self` and run in one transaction together with their search update.

mod export_log;
mod frame;
mod import;
mod meta;
mod recipe;
mod roll;
mod search;
mod source;
mod tag;
mod verify;

use std::str::FromStr;

use gtp_core::{AppError, ErrorCode};
use rusqlite::types::Type;
use rusqlite::Row;

pub use export_log::ExportRecord;
pub use search::{MAX_SEARCH_CHARS, MAX_SEARCH_TAGS};
pub use tag::{FrameTags, MAX_TAGGED_FRAMES, MAX_TAG_NAMES};
pub use verify::RecordedOriginal;

/// SQL expression for "now" as RFC 3339 UTC text.
pub(crate) const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ', 'now')";

/// Longest name (roll, camera, film) accepted, in characters.
pub const MAX_NAME_CHARS: usize = 200;
/// Longest note accepted, in characters.
pub const MAX_NOTE_CHARS: usize = 10_000;

/// Reads a text column into a type parsed from text (ids, enums).
pub(crate) fn parse_col<T: FromStr<Err = AppError>>(
    row: &Row<'_>,
    idx: usize,
) -> rusqlite::Result<T> {
    let text: String = row.get(idx)?;
    text.parse().map_err(|err: AppError| {
        rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(err))
    })
}

pub(crate) fn not_found(what: &str, id: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::NotFound, format!("That {what} does not exist."))
        .with_details(id.to_string())
}

/// Trims a required name and checks its length.
pub(crate) fn clean_name(name: &str, what: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            format!("The {what} needs a name."),
        ));
    }
    check_len(name, MAX_NAME_CHARS, what)?;
    Ok(name.to_owned())
}

/// Trims an optional text; empty becomes `None`.
pub(crate) fn clean_optional(text: Option<&str>, what: &str) -> Result<Option<String>, AppError> {
    match text.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => {
            check_len(t, MAX_NAME_CHARS, what)?;
            Ok(Some(t.to_owned()))
        }
        None => Ok(None),
    }
}

pub(crate) fn check_len(text: &str, max: usize, what: &str) -> Result<(), AppError> {
    if text.chars().count() > max {
        return Err(
            AppError::new(ErrorCode::InvalidInput, format!("The {what} is too long."))
                .with_details(format!("at most {max} characters")),
        );
    }
    Ok(())
}
