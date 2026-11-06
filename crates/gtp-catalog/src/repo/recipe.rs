//! The edit recipe of a frame, stored as versioned JSON and migrated when read.

use rusqlite::{params, OptionalExtension, Transaction};

use gtp_core::{migrate, AppError, ErrorCode, FrameId, Recipe};

use super::{not_found, NOW};
use crate::db::db_error;
use crate::Catalog;

/// Writes one recipe and stamps the frame's `edited_at`, inside the caller's transaction.
fn save_in(tx: &Transaction<'_>, frame: FrameId, recipe: &Recipe) -> Result<(), AppError> {
    let json = serde_json::to_string(recipe).map_err(|err| {
        AppError::new(ErrorCode::Internal, "The edit could not be saved.")
            .with_details(err.to_string())
    })?;
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
    Ok(())
}

impl Catalog {
    /// Saves a frame's recipe and stamps the frame's `edited_at`.
    pub fn save_recipe(&mut self, frame: FrameId, recipe: &Recipe) -> Result<(), AppError> {
        let tx = self.conn.transaction().map_err(db_error)?;
        save_in(&tx, frame, recipe)?;
        tx.commit().map_err(db_error)
    }

    /// Saves several frames' recipes in one transaction: an unknown frame saves none of them.
    pub fn save_recipes(&mut self, items: &[(FrameId, Recipe)]) -> Result<(), AppError> {
        let tx = self.conn.transaction().map_err(db_error)?;
        for (frame, recipe) in items {
            save_in(&tx, *frame, recipe)?;
        }
        tx.commit().map_err(db_error)
    }

    /// Writes the colour axes `[red_cyan, green_magenta, blue_yellow]` into the recipes of those of
    /// `frames` that were never edited by the user (`edited_at` empty), leaving the rest of each
    /// recipe and `edited_at` as they are: the automatic recipe changes, the frame does not count
    /// as edited. Returns the frames whose recipe changed. One transaction.
    pub fn set_unedited_color_axes(
        &mut self,
        frames: &[FrameId],
        axes: [f32; 3],
    ) -> Result<Vec<FrameId>, AppError> {
        let tx = self.conn.transaction().map_err(db_error)?;
        let mut changed = Vec::new();
        for frame in frames {
            let found: Option<String> = tx
                .query_row(
                    "SELECT recipe.json FROM recipe JOIN frame ON frame.id = recipe.frame_id
                     WHERE recipe.frame_id = ?1 AND frame.edited_at IS NULL",
                    [frame.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(db_error)?;
            let Some(json) = found else { continue };
            let mut recipe = migrate(&json)?;
            let current = [
                recipe.color.red_cyan,
                recipe.color.green_magenta,
                recipe.color.blue_yellow,
            ];
            if current == axes {
                continue;
            }
            [
                recipe.color.red_cyan,
                recipe.color.green_magenta,
                recipe.color.blue_yellow,
            ] = axes;
            let json = serde_json::to_string(&recipe).map_err(|err| {
                AppError::new(
                    ErrorCode::Internal,
                    "The colour balance could not be saved.",
                )
                .with_details(err.to_string())
            })?;
            tx.execute(
                &format!(
                    "UPDATE recipe SET schema_v = ?2, json = ?3, updated_at = {NOW}
                     WHERE frame_id = ?1"
                ),
                params![frame.to_string(), recipe.v, json],
            )
            .map_err(db_error)?;
            changed.push(*frame);
        }
        tx.commit().map_err(db_error)?;
        Ok(changed)
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
