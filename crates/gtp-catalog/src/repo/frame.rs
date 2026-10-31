//! Frames of a roll.

use rusqlite::{params, OptionalExtension, Row};

use gtp_core::{AppError, ErrorCode, Frame, FrameId, FrameKind, RollId};

use super::{check_len, not_found, parse_col, MAX_NOTE_CHARS, NOW};
use crate::db::db_error;
use crate::{fts, Catalog};

const COLUMNS: &str =
    "id, roll_id, index_in_roll, kind, width, height, has_ir, note, created_at, edited_at";

fn kind_text(kind: FrameKind) -> &'static str {
    match kind {
        FrameKind::ScanRgbIr => "ScanRgbIr",
        FrameKind::RgbTriplet => "RgbTriplet",
    }
}

fn read(row: &Row<'_>) -> rusqlite::Result<Frame> {
    let kind: String = row.get(3)?;
    let kind = match kind.as_str() {
        "ScanRgbIr" => FrameKind::ScanRgbIr,
        "RgbTriplet" => FrameKind::RgbTriplet,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                format!("unknown frame kind {other:?}").into(),
            ))
        }
    };
    Ok(Frame {
        id: parse_col(row, 0)?,
        roll_id: parse_col(row, 1)?,
        index_in_roll: row.get(2)?,
        kind,
        width: row.get(4)?,
        height: row.get(5)?,
        has_ir: row.get(6)?,
        note: row.get(7)?,
        created_at: row.get(8)?,
        edited_at: row.get(9)?,
    })
}

impl Catalog {
    /// Adds a frame to its roll; `created_at` is set by the catalog and `edited_at` starts empty.
    pub fn insert_frame(&mut self, frame: &Frame) -> Result<Frame, AppError> {
        let note = frame.note.trim();
        check_len(note, MAX_NOTE_CHARS, "note")?;
        let tx = self.conn.transaction().map_err(db_error)?;
        tx.execute(
            &format!(
                "INSERT INTO frame({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, {NOW}, NULL)"
            ),
            params![
                frame.id.to_string(),
                frame.roll_id.to_string(),
                frame.index_in_roll,
                kind_text(frame.kind),
                frame.width,
                frame.height,
                frame.has_ir,
                note
            ],
        )
        .map_err(|err| match err.sqlite_error_code() {
            Some(rusqlite::ErrorCode::ConstraintViolation) => AppError::new(
                ErrorCode::InvalidInput,
                "The frame's roll does not exist, or the frame is already there.",
            )
            .with_details(err.to_string()),
            _ => db_error(err),
        })?;
        fts::refresh_frame(&tx, frame.id)?;
        tx.commit().map_err(db_error)?;
        self.get_frame(frame.id)
    }

    pub fn get_frame(&self, id: FrameId) -> Result<Frame, AppError> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM frame WHERE id = ?1"),
                [id.to_string()],
                read,
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| not_found("frame", id))
    }

    /// The frames of a roll in roll order.
    pub fn list_frames(&self, roll: RollId) -> Result<Vec<Frame>, AppError> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM frame WHERE roll_id = ?1 ORDER BY index_in_roll, rowid"
            ))
            .map_err(db_error)?;
        let frames = stmt
            .query_map([roll.to_string()], read)
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error);
        frames
    }

    /// One page of frames: those of `roll`, or of every roll (newest roll first), in roll order.
    pub fn list_frames_page(
        &self,
        roll: Option<RollId>,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Frame>, AppError> {
        let columns = COLUMNS
            .split(", ")
            .map(|c| format!("f.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {columns} FROM frame f JOIN roll r ON r.id = f.roll_id
                 WHERE ?1 IS NULL OR f.roll_id = ?1
                 ORDER BY r.created_at DESC, r.rowid DESC, f.index_in_roll, f.rowid
                 LIMIT ?2 OFFSET ?3"
            ))
            .map_err(db_error)?;
        let frames = stmt
            .query_map(params![roll.map(|r| r.to_string()), limit, offset], read)
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error);
        frames
    }

    /// How many frames `roll` has, or the library when `None`.
    pub fn count_frames(&self, roll: Option<RollId>) -> Result<u32, AppError> {
        self.conn
            .query_row(
                "SELECT count(*) FROM frame WHERE ?1 IS NULL OR roll_id = ?1",
                [roll.map(|r| r.to_string())],
                |row| row.get(0),
            )
            .map_err(db_error)
    }

    pub fn set_frame_note(&mut self, id: FrameId, note: &str) -> Result<Frame, AppError> {
        let note = note.trim();
        check_len(note, MAX_NOTE_CHARS, "note")?;
        let tx = self.conn.transaction().map_err(db_error)?;
        let changed = tx
            .execute(
                "UPDATE frame SET note = ?2 WHERE id = ?1",
                params![id.to_string(), note],
            )
            .map_err(db_error)?;
        if changed == 0 {
            return Err(not_found("frame", id));
        }
        fts::refresh_frame(&tx, id)?;
        tx.commit().map_err(db_error)?;
        self.get_frame(id)
    }

    /// Deletes frames with their sources, recipes and tag links; unknown ids are skipped. Returns
    /// how many were deleted.
    pub fn delete_frames(&mut self, ids: &[FrameId]) -> Result<usize, AppError> {
        let tx = self.conn.transaction().map_err(db_error)?;
        let mut deleted = 0;
        for id in ids {
            fts::forget_frame(&tx, *id)?;
            deleted += tx
                .execute("DELETE FROM frame WHERE id = ?1", [id.to_string()])
                .map_err(db_error)?;
        }
        tx.commit().map_err(db_error)?;
        Ok(deleted)
    }
}
