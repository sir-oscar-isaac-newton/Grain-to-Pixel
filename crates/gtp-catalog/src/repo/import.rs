//! Recording an imported frame: the frame, its original files, its starting recipe and its search
//! text in one transaction, so a frame is never in the catalog without its sources.

use rusqlite::params;

use gtp_core::{AppError, ErrorCode, Frame, FrameKind, Recipe, SourceFile};

use super::source::role_text;
use super::{check_len, MAX_NOTE_CHARS, NOW};
use crate::db::db_error;
use crate::{fts, Catalog};

impl Catalog {
    /// Adds an imported frame as the next frame of its roll (`index_in_roll` is chosen here, so
    /// two imports into one roll never collide), with its sources and starting recipe. The frame
    /// is not marked edited. Returns the stored frame.
    pub fn insert_imported_frame(
        &mut self,
        frame: &Frame,
        sources: &[SourceFile],
        recipe: &Recipe,
    ) -> Result<Frame, AppError> {
        self.store_new_frame(frame, sources, recipe, None, None)
    }

    /// [`Catalog::insert_imported_frame`] for a frame the scanner made: it also records the frame
    /// number on the roll as the scanner counts it, and how the frame was scanned (`info`, JSON), in
    /// the same transaction.
    pub fn insert_scanned_frame(
        &mut self,
        frame: &Frame,
        sources: &[SourceFile],
        recipe: &Recipe,
        scan_number: u32,
        info: &str,
    ) -> Result<Frame, AppError> {
        self.store_new_frame(frame, sources, recipe, Some((scan_number, info)), None)
    }

    /// [`Catalog::insert_imported_frame`] for a frame the camera captured: it also records how the
    /// frame was lit (`info`, JSON: levels, their source and the exposures), in the same
    /// transaction.
    pub fn insert_captured_frame(
        &mut self,
        frame: &Frame,
        sources: &[SourceFile],
        recipe: &Recipe,
        info: &str,
    ) -> Result<Frame, AppError> {
        self.store_new_frame(frame, sources, recipe, None, Some(info))
    }

    /// How a captured frame was lit, as the JSON `insert_captured_frame` stored; `None` for a frame
    /// that was imported or scanned (or captured before H-15). A frame that does not exist is
    /// `NotFound`.
    pub fn capture_info(&self, frame: gtp_core::FrameId) -> Result<Option<String>, AppError> {
        self.conn
            .query_row(
                "SELECT capture_info FROM frame WHERE id = ?1",
                params![frame.to_string()],
                |row| row.get(0),
            )
            .map_err(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => {
                    AppError::new(ErrorCode::NotFound, "That frame does not exist.")
                }
                other => db_error(other),
            })
    }

    /// The highest scanner frame number among the roll's scanned frames; `None` when none of its
    /// frames was scanned (imported frames have none).
    pub fn last_scan_number(&self, roll: gtp_core::RollId) -> Result<Option<u32>, AppError> {
        let last: Option<i64> = self
            .conn
            .query_row(
                "SELECT max(scan_number) FROM frame WHERE roll_id = ?1",
                params![roll.to_string()],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        Ok(last.and_then(|n| u32::try_from(n).ok()))
    }

    fn store_new_frame(
        &mut self,
        frame: &Frame,
        sources: &[SourceFile],
        recipe: &Recipe,
        scan: Option<(u32, &str)>,
        capture: Option<&str>,
    ) -> Result<Frame, AppError> {
        check_len(frame.note.trim(), MAX_NOTE_CHARS, "note")?;
        if sources.iter().any(|s| s.frame_id != frame.id) {
            return Err(AppError::internal("a source file belongs to another frame"));
        }
        let json = serde_json::to_string(recipe).map_err(AppError::internal)?;
        let kind = match frame.kind {
            FrameKind::ScanRgbIr => "ScanRgbIr",
            FrameKind::RgbTriplet => "RgbTriplet",
        };
        let id = frame.id.to_string();
        let tx = self.conn.transaction().map_err(db_error)?;
        tx.execute(
            &format!(
                "INSERT INTO frame(id, roll_id, index_in_roll, kind, width, height, has_ir, note,
                                   created_at, edited_at, scan_number, scan_info, capture_info)
                 SELECT ?1, ?2, coalesce(max(index_in_roll), 0) + 1, ?3, ?4, ?5, ?6, ?7, {NOW}, NULL,
                        ?8, ?9, ?10
                 FROM frame WHERE roll_id = ?2"
            ),
            params![
                id,
                frame.roll_id.to_string(),
                kind,
                frame.width,
                frame.height,
                frame.has_ir,
                frame.note.trim(),
                scan.map(|(number, _)| number),
                scan.map(|(_, info)| info),
                capture
            ],
        )
        .map_err(|err| match err.sqlite_error_code() {
            Some(rusqlite::ErrorCode::ConstraintViolation) => AppError::new(
                ErrorCode::NotFound,
                "The roll to import into does not exist.",
            )
            .with_details(err.to_string()),
            _ => db_error(err),
        })?;
        for source in sources {
            let bytes = i64::try_from(source.bytes).map_err(AppError::internal)?;
            tx.execute(
                "INSERT INTO source_file(frame_id, role, rel_path, sha256, bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    role_text(source.role),
                    source.rel_path,
                    source.sha256,
                    bytes
                ],
            )
            .map_err(db_error)?;
        }
        tx.execute(
            &format!(
                "INSERT INTO recipe(frame_id, schema_v, json, updated_at) VALUES (?1, ?2, ?3, {NOW})"
            ),
            params![id, recipe.v, json],
        )
        .map_err(db_error)?;
        fts::refresh_frame(&tx, frame.id)?;
        tx.commit().map_err(db_error)?;
        self.get_frame(frame.id)
    }
}
