//! Frames of a roll.

use std::collections::HashMap;

use rusqlite::{params, params_from_iter, OptionalExtension, Row};

use gtp_core::{AppError, ErrorCode, Frame, FrameFlag, FrameId, FrameKind, RollId};

use super::{check_len, not_found, parse_col, MAX_NOTE_CHARS, NOW};
use crate::db::db_error;
use crate::{fts, Catalog};

const COLUMNS: &str =
    "id, roll_id, index_in_roll, kind, width, height, has_ir, note, created_at, edited_at, flag";

/// The columns an insert sets; the flag starts as the column default (`none`).
const INSERT_COLUMNS: &str =
    "id, roll_id, index_in_roll, kind, width, height, has_ir, note, created_at, edited_at";

/// Joins the time of each frame's newest export as `x.exported_at` (a frame never exported has no
/// row). `export_log` has no index on `frame_id`, so this reads the log once, not once per frame.
pub(super) const LAST_EXPORT_JOIN: &str =
    "LEFT JOIN (SELECT frame_id, MAX(created_at) AS exported_at
     FROM export_log GROUP BY frame_id) x ON x.frame_id = f.id";

/// True for a frame exported and edited afterwards. Both times come from the catalog's `NOW`
/// (`YYYY-MM-DDTHH:MM:SSZ`), so comparing them as text is comparing them in time. A frame never
/// exported (`x.exported_at` NULL) or never edited (`f.edited_at` NULL) gives NULL, so it is out.
pub(super) const NEEDS_EXPORT: &str = "f.edited_at > x.exported_at";

/// The frame columns of COLUMNS, each prefixed with the alias `f`..
pub(super) fn prefixed_columns() -> String {
    COLUMNS
        .split(", ")
        .map(|c| format!("f.{c}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn kind_text(kind: FrameKind) -> &'static str {
    match kind {
        FrameKind::ScanRgbIr => "ScanRgbIr",
        FrameKind::RgbTriplet => "RgbTriplet",
    }
}

pub(super) fn read(row: &Row<'_>) -> rusqlite::Result<Frame> {
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
    let flag: String = row.get(10)?;
    let flag = FrameFlag::parse(&flag).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            10,
            rusqlite::types::Type::Text,
            format!("unknown frame flag {flag:?}").into(),
        )
    })?;
    Ok(Frame {
        id: parse_col(row, 0)?,
        roll_id: parse_col(row, 1)?,
        index_in_roll: row.get(2)?,
        kind,
        width: row.get(4)?,
        height: row.get(5)?,
        has_ir: row.get(6)?,
        note: row.get(7)?,
        flag,
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
                "INSERT INTO frame({INSERT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, {NOW}, NULL)"
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

    /// One page of frames: those of `roll`, or of every roll (newest roll first), in roll order;
    /// with `flag`, only the frames carrying that mark; with `needs_export`, only the frames
    /// exported and edited since their newest export.
    pub fn list_frames_page(
        &self,
        roll: Option<RollId>,
        flag: Option<FrameFlag>,
        needs_export: bool,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Frame>, AppError> {
        let columns = prefixed_columns();
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {columns} FROM frame f JOIN roll r ON r.id = f.roll_id {LAST_EXPORT_JOIN}
                 WHERE (?1 IS NULL OR f.roll_id = ?1) AND (?2 IS NULL OR f.flag = ?2)
                   AND (?3 = 0 OR {NEEDS_EXPORT})
                 ORDER BY r.created_at DESC, r.rowid DESC, f.index_in_roll, f.rowid
                 LIMIT ?4 OFFSET ?5"
            ))
            .map_err(db_error)?;
        let frames = stmt
            .query_map(
                params![
                    roll.map(|r| r.to_string()),
                    flag.map(FrameFlag::as_str),
                    needs_export,
                    limit,
                    offset
                ],
                read,
            )
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error);
        frames
    }

    /// How many frames `roll` has, or the library when `None`; with `flag`, only those carrying
    /// that mark; with `needs_export`, only those exported and edited since their newest export.
    pub fn count_frames(
        &self,
        roll: Option<RollId>,
        flag: Option<FrameFlag>,
        needs_export: bool,
    ) -> Result<u32, AppError> {
        self.conn
            .query_row(
                &format!(
                    "SELECT count(*) FROM frame f {LAST_EXPORT_JOIN}
                     WHERE (?1 IS NULL OR f.roll_id = ?1) AND (?2 IS NULL OR f.flag = ?2)
                       AND (?3 = 0 OR {NEEDS_EXPORT})"
                ),
                params![
                    roll.map(|r| r.to_string()),
                    flag.map(FrameFlag::as_str),
                    needs_export
                ],
                |row| row.get(0),
            )
            .map_err(db_error)
    }

    /// The time of the newest logged export of each of `ids` that was ever exported (frames never
    /// exported are absent). Times are in the catalog's `NOW` format.
    pub fn last_exports(&self, ids: &[FrameId]) -> Result<HashMap<FrameId, String>, AppError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let marks = vec!["?"; ids.len()].join(", ");
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT frame_id, MAX(created_at) FROM export_log
                 WHERE frame_id IN ({marks}) GROUP BY frame_id"
            ))
            .map_err(db_error)?;
        let rows = stmt
            .query_map(
                params_from_iter(ids.iter().map(ToString::to_string)),
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .map_err(db_error)?;
        let mut found = HashMap::new();
        for row in rows {
            let (id, at) = row.map_err(db_error)?;
            if let (Ok(id), Some(at)) = (id.parse::<FrameId>(), at) {
                found.insert(id, at);
            }
        }
        Ok(found)
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

    /// Sets the pick or reject mark of frames, in one transaction, and returns the frames that
    /// exist, in the order of `ids` (unknown ids are skipped, a repeated id is returned once).
    /// A flag is a judgement, not an edit of the picture, so `edited_at` is left unchanged.
    pub fn set_frames_flag(
        &mut self,
        ids: &[FrameId],
        flag: FrameFlag,
    ) -> Result<Vec<Frame>, AppError> {
        let tx = self.conn.transaction().map_err(db_error)?;
        let mut found: Vec<FrameId> = Vec::new();
        for id in ids {
            let changed = tx
                .execute(
                    "UPDATE frame SET flag = ?2 WHERE id = ?1",
                    params![id.to_string(), flag.as_str()],
                )
                .map_err(db_error)?;
            if changed > 0 && !found.contains(id) {
                found.push(*id);
            }
        }
        tx.commit().map_err(db_error)?;
        found.into_iter().map(|id| self.get_frame(id)).collect()
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
