//! The flat field of a roll (P-21).

use gtp_core::{AppError, ErrorCode, ExposureInfo, FlatField, RollId};
use rusqlite::{params, OptionalExtension};

use super::NOW;
use crate::db::db_error;
use crate::Catalog;

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message.to_owned())
}

impl Catalog {
    /// Stores the roll's flat field, replacing the one it has.
    pub fn set_flat_field(&mut self, roll: RollId, flat: &FlatField) -> Result<(), AppError> {
        if !flat.is_valid() {
            return Err(invalid("A flat field needs a grid of usable light values."));
        }
        let light: Vec<u8> = flat.light.iter().flat_map(|v| v.to_le_bytes()).collect();
        let sources = serde_json::to_string(&flat.sources)
            .map_err(|err| AppError::internal(err.to_string()))?;
        // Nothing known and nothing kept are stored as NULL, as rows made before they existed have.
        let exposure = (!flat.exposure.is_empty())
            .then(|| serde_json::to_string(&flat.exposure))
            .transpose()
            .map_err(|err| AppError::internal(err.to_string()))?;
        let files = (!flat.files.is_empty())
            .then(|| serde_json::to_string(&flat.files))
            .transpose()
            .map_err(|err| AppError::internal(err.to_string()))?;
        self.get_roll(roll)?;
        let tx = self.conn.transaction().map_err(db_error)?;
        tx.execute(
            "DELETE FROM flat_field WHERE roll_id = ?1",
            [roll.to_string()],
        )
        .map_err(db_error)?;
        tx.execute(
            &format!(
                "INSERT INTO flat_field(roll_id, grid_w, grid_h, frame_w, frame_h, light, sources, exposure, files, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, {NOW})"
            ),
            params![
                roll.to_string(),
                flat.grid_w,
                flat.grid_h,
                flat.frame_w,
                flat.frame_h,
                light,
                sources,
                exposure,
                files
            ],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)
    }

    /// The roll's flat field, if it has one.
    pub fn get_flat_field(&self, roll: RollId) -> Result<Option<FlatField>, AppError> {
        self.conn
            .query_row(
                "SELECT grid_w, grid_h, frame_w, frame_h, light, sources, exposure, files FROM flat_field
                 WHERE roll_id = ?1",
                [roll.to_string()],
                |row| {
                    let light: Vec<u8> = row.get(4)?;
                    let sources: String = row.get(5)?;
                    let exposure: Option<String> = row.get(6)?;
                    let files: Option<String> = row.get(7)?;
                    Ok(FlatField {
                        grid_w: row.get(0)?,
                        grid_h: row.get(1)?,
                        frame_w: row.get(2)?,
                        frame_h: row.get(3)?,
                        light: light
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|b| f32::from_le_bytes(*b))
                            .collect(),
                        sources: serde_json::from_str(&sources).unwrap_or_default(),
                        exposure: exposure
                            .and_then(|json| serde_json::from_str::<ExposureInfo>(&json).ok())
                            .unwrap_or_default(),
                        files: files
                            .and_then(|json| serde_json::from_str(&json).ok())
                            .unwrap_or_default(),
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    /// Removes the roll's flat field; true when it had one. A roll that does not exist is not found.
    pub fn clear_flat_field(&mut self, roll: RollId) -> Result<bool, AppError> {
        self.get_roll(roll)?;
        let removed = self
            .conn
            .execute(
                "DELETE FROM flat_field WHERE roll_id = ?1",
                [roll.to_string()],
            )
            .map_err(db_error)?;
        Ok(removed > 0)
    }
}
