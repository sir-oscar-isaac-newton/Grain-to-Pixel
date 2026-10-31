//! Rolls. Changing a roll rewrites the search text of all its frames.

use rusqlite::{params, OptionalExtension, Row};

use gtp_core::model::{check_iso, check_shot_range};
use gtp_core::{AppError, FilmFormat, Roll, RollId};

use super::{check_len, clean_name, clean_optional, not_found, parse_col, MAX_NOTE_CHARS, NOW};
use crate::db::db_error;
use crate::{fts, Catalog};

const COLUMNS: &str = "id, name, film_stock, format, camera, iso_box, iso_shot, shot_from, shot_to,
    notes, film_base_r, film_base_g, film_base_b, created_at, film_base_confidence";

fn read(row: &Row<'_>) -> rusqlite::Result<Roll> {
    let base: [Option<f32>; 3] = [row.get(10)?, row.get(11)?, row.get(12)?];
    Ok(Roll {
        id: parse_col(row, 0)?,
        name: row.get(1)?,
        film_stock: row.get(2)?,
        // Only 35 mm exists; the column is kept for later formats.
        format: FilmFormat::Mm35,
        camera: row.get(4)?,
        iso_box: row.get(5)?,
        iso_shot: row.get(6)?,
        shot_from: row.get(7)?,
        shot_to: row.get(8)?,
        notes: row.get(9)?,
        film_base_rgb: match base {
            [Some(r), Some(g), Some(b)] => Some([r, g, b]),
            _ => None,
        },
        film_base_confidence: row.get(14)?,
        created_at: row.get(13)?,
    })
}

/// A film base must be three finite values above 0, a confidence a number from 0 to 1, and a
/// confidence only makes sense with a base.
fn check_base(rgb: Option<[f32; 3]>, confidence: Option<f32>) -> Result<(), AppError> {
    let bad = |message: &str, details: String| {
        AppError::new(gtp_core::ErrorCode::InvalidInput, message.to_owned()).with_details(details)
    };
    if let Some(rgb) = rgb {
        if !rgb.iter().all(|v| v.is_finite() && *v > 0.0) {
            return Err(bad(
                "A film base needs three values above 0.",
                format!("{rgb:?}"),
            ));
        }
    }
    match confidence {
        Some(c) if !(0.0..=1.0).contains(&c) => Err(bad(
            "The film base confidence must be between 0 and 1.",
            format!("{c}"),
        )),
        Some(_) if rgb.is_none() => Err(bad(
            "A film base confidence needs a film base.",
            String::new(),
        )),
        _ => Ok(()),
    }
}

/// The roll with its texts trimmed and every field checked.
fn validated(roll: &Roll) -> Result<Roll, AppError> {
    check_base(roll.film_base_rgb, roll.film_base_confidence)?;
    check_iso(roll.iso_box)?;
    check_iso(roll.iso_shot)?;
    let shot_from = clean_optional(roll.shot_from.as_deref(), "start date")?;
    let shot_to = clean_optional(roll.shot_to.as_deref(), "end date")?;
    check_shot_range(shot_from.as_deref(), shot_to.as_deref())?;
    let notes = roll.notes.trim().to_owned();
    check_len(&notes, MAX_NOTE_CHARS, "note")?;
    Ok(Roll {
        name: clean_name(&roll.name, "roll")?,
        film_stock: clean_optional(roll.film_stock.as_deref(), "film name")?,
        camera: clean_optional(roll.camera.as_deref(), "camera name")?,
        shot_from,
        shot_to,
        notes,
        ..roll.clone()
    })
}

impl Catalog {
    /// Adds a roll; `created_at` is set by the catalog. Returns the stored roll.
    pub fn insert_roll(&mut self, roll: &Roll) -> Result<Roll, AppError> {
        let r = validated(roll)?;
        let base = r.film_base_rgb.map_or([None; 3], |b| b.map(Some));
        self.conn
            .execute(
                &format!(
                    "INSERT INTO roll({COLUMNS}) VALUES
                     (?1, ?2, ?3, '35mm', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, {NOW}, ?13)"
                ),
                params![
                    r.id.to_string(),
                    r.name,
                    r.film_stock,
                    r.camera,
                    r.iso_box,
                    r.iso_shot,
                    r.shot_from,
                    r.shot_to,
                    r.notes,
                    base[0],
                    base[1],
                    base[2],
                    r.film_base_confidence
                ],
            )
            .map_err(db_error)?;
        self.get_roll(r.id)
    }

    /// Stores `rgb` as the roll's film base only if the roll has none yet, so a base set by hand or
    /// by an earlier frame is never overwritten. Returns the roll as stored and whether this call
    /// set the base. Safe against two imports racing: the check and the write are one statement.
    pub fn set_roll_film_base_if_unset(
        &mut self,
        id: RollId,
        rgb: [f32; 3],
        confidence: Option<f32>,
    ) -> Result<(Roll, bool), AppError> {
        check_base(Some(rgb), confidence)?;
        let changed = self
            .conn
            .execute(
                "UPDATE roll SET film_base_r = ?2, film_base_g = ?3, film_base_b = ?4,
                 film_base_confidence = ?5
                 WHERE id = ?1 AND (film_base_r IS NULL OR film_base_g IS NULL OR film_base_b IS NULL)",
                params![id.to_string(), rgb[0], rgb[1], rgb[2], confidence],
            )
            .map_err(db_error)?;
        Ok((self.get_roll(id)?, changed > 0))
    }

    /// Replaces the editable fields of a roll (everything but `id` and `created_at`).
    pub fn update_roll(&mut self, roll: &Roll) -> Result<Roll, AppError> {
        let r = validated(roll)?;
        let base = r.film_base_rgb.map_or([None; 3], |b| b.map(Some));
        let tx = self.conn.transaction().map_err(db_error)?;
        let changed = tx
            .execute(
                "UPDATE roll SET name = ?2, film_stock = ?3, camera = ?4, iso_box = ?5,
                 iso_shot = ?6, shot_from = ?7, shot_to = ?8, notes = ?9,
                 film_base_r = ?10, film_base_g = ?11, film_base_b = ?12,
                 film_base_confidence = ?13 WHERE id = ?1",
                params![
                    r.id.to_string(),
                    r.name,
                    r.film_stock,
                    r.camera,
                    r.iso_box,
                    r.iso_shot,
                    r.shot_from,
                    r.shot_to,
                    r.notes,
                    base[0],
                    base[1],
                    base[2],
                    r.film_base_confidence
                ],
            )
            .map_err(db_error)?;
        if changed == 0 {
            return Err(not_found("roll", r.id));
        }
        fts::refresh_roll(&tx, r.id)?;
        tx.commit().map_err(db_error)?;
        self.get_roll(r.id)
    }

    pub fn get_roll(&self, id: RollId) -> Result<Roll, AppError> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM roll WHERE id = ?1"),
                [id.to_string()],
                read,
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| not_found("roll", id))
    }

    /// All rolls, newest first.
    pub fn list_rolls(&self) -> Result<Vec<Roll>, AppError> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM roll ORDER BY created_at DESC, rowid DESC"
            ))
            .map_err(db_error)?;
        let rolls = stmt
            .query_map([], read)
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error);
        rolls
    }

    /// Deletes a roll with its frames, sources, recipes and tag links.
    pub fn delete_roll(&mut self, id: RollId) -> Result<(), AppError> {
        let tx = self.conn.transaction().map_err(db_error)?;
        fts::forget_roll(&tx, id)?;
        let deleted = tx
            .execute("DELETE FROM roll WHERE id = ?1", [id.to_string()])
            .map_err(db_error)?;
        if deleted == 0 {
            return Err(not_found("roll", id));
        }
        tx.commit().map_err(db_error)
    }
}
