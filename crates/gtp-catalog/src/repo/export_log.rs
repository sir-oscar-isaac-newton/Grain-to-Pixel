//! The export log: one row per exported file. Rows are kept when the frame is deleted.

use rusqlite::params;

use gtp_core::{AppError, FrameId};

use super::NOW;
use crate::db::db_error;
use crate::Catalog;

/// One exported file, as logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRecord {
    pub frame_id: String,
    pub format: String,
    pub path: String,
    pub created_at: String,
}

impl Catalog {
    /// Logs a file an export wrote.
    pub fn log_export(&mut self, frame: FrameId, format: &str, path: &str) -> Result<(), AppError> {
        self.conn
            .execute(
                &format!(
                    "INSERT INTO export_log(frame_id, format, path, created_at) VALUES (?1, ?2, ?3, {NOW})"
                ),
                params![frame.to_string(), format, path],
            )
            .map_err(db_error)?;
        Ok(())
    }

    /// The logged exports of a frame, oldest first.
    pub fn exports_of(&self, frame: FrameId) -> Result<Vec<ExportRecord>, AppError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT frame_id, format, path, created_at FROM export_log
                 WHERE frame_id = ?1 ORDER BY id",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([frame.to_string()], |row| {
                Ok(ExportRecord {
                    frame_id: row.get(0)?,
                    format: row.get(1)?,
                    path: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }
}
