//! Original files of a frame.

use rusqlite::{params, Row};

use gtp_core::{AppError, ErrorCode, FrameId, SourceFile, SourceRole};

use super::parse_col;
use crate::db::db_error;
use crate::Catalog;

fn role_text(role: SourceRole) -> &'static str {
    match role {
        SourceRole::Rgb => "rgb",
        SourceRole::Ir => "ir",
        SourceRole::R => "r",
        SourceRole::G => "g",
        SourceRole::B => "b",
    }
}

fn read(row: &Row<'_>) -> rusqlite::Result<SourceFile> {
    let role: String = row.get(1)?;
    let role = match role.as_str() {
        "rgb" => SourceRole::Rgb,
        "ir" => SourceRole::Ir,
        "r" => SourceRole::R,
        "g" => SourceRole::G,
        "b" => SourceRole::B,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                format!("unknown source role {other:?}").into(),
            ))
        }
    };
    Ok(SourceFile {
        frame_id: parse_col(row, 0)?,
        role,
        rel_path: row.get(2)?,
        sha256: row.get(3)?,
        bytes: row.get::<_, i64>(4)?.try_into().map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Integer,
                Box::new(err),
            )
        })?,
    })
}

impl Catalog {
    /// Records an original file of a frame (one per role).
    pub fn insert_source(&mut self, source: &SourceFile) -> Result<(), AppError> {
        let bytes = i64::try_from(source.bytes).map_err(|_| {
            AppError::new(ErrorCode::InvalidInput, "That file is too large.")
                .with_details(source.bytes.to_string())
        })?;
        self.conn
            .execute(
                "INSERT INTO source_file(frame_id, role, rel_path, sha256, bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    source.frame_id.to_string(),
                    role_text(source.role),
                    source.rel_path,
                    source.sha256,
                    bytes
                ],
            )
            .map(|_| ())
            .map_err(db_error)
    }

    pub fn sources_of(&self, frame: FrameId) -> Result<Vec<SourceFile>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT frame_id, role, rel_path, sha256, bytes FROM source_file
                 WHERE frame_id = ?1 ORDER BY role",
            )
            .map_err(db_error)?;
        let sources = stmt
            .query_map([frame.to_string()], read)
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error);
        sources
    }
}
