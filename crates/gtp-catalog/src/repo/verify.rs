//! What a library verify needs from the catalog: every recorded original with its frame and roll,
//! and every frame's key. Read-only.

use rusqlite::Row;

use gtp_core::{AppError, FrameId, RollId, SourceFile};

use super::parse_col;
use crate::db::db_error;
use crate::Catalog;

/// An original file with the frame and roll it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedOriginal {
    pub roll_id: RollId,
    pub source: SourceFile,
}

fn read(row: &Row<'_>) -> rusqlite::Result<RecordedOriginal> {
    let role: String = row.get(2)?;
    let role = super::source::role_from_text(&role).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            format!("unknown source role {role:?}").into(),
        )
    })?;
    let bytes: i64 = row.get(5)?;
    Ok(RecordedOriginal {
        roll_id: parse_col(row, 0)?,
        source: SourceFile {
            frame_id: parse_col(row, 1)?,
            role,
            rel_path: row.get(3)?,
            sha256: row.get(4)?,
            bytes: u64::try_from(bytes).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Integer,
                    Box::new(err),
                )
            })?,
        },
    })
}

impl Catalog {
    /// Every recorded original, newest roll first, then roll order, scan before infrared.
    pub fn recorded_originals(&self) -> Result<Vec<RecordedOriginal>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT f.roll_id, s.frame_id, s.role, s.rel_path, s.sha256, s.bytes
                 FROM source_file s JOIN frame f ON f.id = s.frame_id JOIN roll r ON r.id = f.roll_id
                 ORDER BY r.created_at DESC, r.rowid DESC, f.index_in_roll, f.rowid, s.role",
            )
            .map_err(db_error)?;
        let originals = stmt
            .query_map([], read)
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error);
        originals
    }

    /// The roll and frame id of every frame in the library.
    pub fn frame_keys(&self) -> Result<Vec<(RollId, FrameId)>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT roll_id, id FROM frame")
            .map_err(db_error)?;
        let keys = stmt
            .query_map([], |row| Ok((parse_col(row, 0)?, parse_col(row, 1)?)))
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error);
        keys
    }
}
