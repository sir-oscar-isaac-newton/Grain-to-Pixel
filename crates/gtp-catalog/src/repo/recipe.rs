//! The edit recipe of a frame, stored as versioned JSON and migrated when read.

use rusqlite::{params, OptionalExtension};

use gtp_core::{migrate, AppError, ErrorCode, FrameId, Recipe};

use super::{not_found, NOW};
use crate::db::db_error;
use crate::Catalog;

impl Catalog {
    /// Saves a frame's recipe and stamps the frame's `edited_at`.
    pub fn save_recipe(&mut self, frame: FrameId, recipe: &Recipe) -> Result<(), AppError> {
        let json = serde_json::to_string(recipe).map_err(|err| {
            AppError::new(ErrorCode::Internal, "The edit could not be saved.")
                .with_details(err.to_string())
        })?;
        let tx = self.conn.transaction().map_err(db_error)?;
        let changed = tx
            .execute(
                &format!("UPDATE frame SET edited_at = {NOW} WHERE id = ?1"),
                [frame.to_string()],
            )
            .map_err(db_error)?;
        if changed == 0 {
            return Err(not_found("frame", frame));
        }
        tx.execute(
            &format!(
                "INSERT INTO recipe(frame_id, schema_v, json, updated_at) VALUES (?1, ?2, ?3, {NOW})
                 ON CONFLICT(frame_id) DO UPDATE SET schema_v = excluded.schema_v,
                 json = excluded.json, updated_at = excluded.updated_at"
            ),
            params![frame.to_string(), recipe.v, json],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)
    }

    /// The frame's recipe, migrated to the current version; `None` when it was never edited.
    pub fn load_recipe(&self, frame: FrameId) -> Result<Option<Recipe>, AppError> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT json FROM recipe WHERE frame_id = ?1",
                [frame.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        json.as_deref().map(migrate).transpose()
    }
}
