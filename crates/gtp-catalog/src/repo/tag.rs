//! Tags on frames and on rolls. A roll's tags count for every frame on it (they are not copied, so
//! frames imported later have them too). Names are unique by their Unicode case-insensitive key.

use rusqlite::{params, Connection, OptionalExtension, Row};

use gtp_core::{AppError, ErrorCode, FrameId, RollId, Tag, TagId};

use super::{not_found, parse_col};
use crate::db::db_error;
use crate::{fts, Catalog};

/// The tags of one frame: its own and those it has from its roll (a tag can be in both).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameTags {
    pub own: Vec<Tag>,
    pub from_roll: Vec<Tag>,
}

fn read(row: &Row<'_>) -> rusqlite::Result<Tag> {
    Ok(Tag {
        id: parse_col(row, 0)?,
        name: row.get(1)?,
    })
}

fn tags(conn: &Connection, sql: &str, id: String) -> Result<Vec<Tag>, AppError> {
    let mut stmt = conn.prepare(sql).map_err(db_error)?;
    let tags = stmt
        .query_map([id], read)
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error);
    tags
}

fn get(conn: &Connection, id: TagId) -> Result<Tag, AppError> {
    conn.query_row(
        "SELECT id, name FROM tag WHERE id = ?1",
        [id.to_string()],
        read,
    )
    .optional()
    .map_err(db_error)?
    .ok_or_else(|| not_found("tag", id))
}

fn duplicate(name: &str) -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "A tag with that name already exists.",
    )
    .with_details(name.to_owned())
}

impl Catalog {
    /// The tag with this name (matched by key), created if there is none yet. An existing tag
    /// keeps its spelling.
    pub fn tag_ensure(&mut self, name: &str) -> Result<Tag, AppError> {
        let key = Tag::key(name)?;
        let existing = self
            .conn
            .query_row("SELECT id, name FROM tag WHERE name_key = ?1", [&key], read)
            .optional()
            .map_err(db_error)?;
        if let Some(tag) = existing {
            return Ok(tag);
        }
        let tag = Tag::new(name)?;
        self.conn
            .execute(
                "INSERT INTO tag(id, name, name_key) VALUES (?1, ?2, ?3)",
                params![tag.id.to_string(), tag.name, key],
            )
            .map_err(db_error)?;
        Ok(tag)
    }

    /// All tags, in name order (case-insensitive).
    pub fn tag_list(&self) -> Result<Vec<Tag>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM tag ORDER BY name_key")
            .map_err(db_error)?;
        let tags = stmt
            .query_map([], read)
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error);
        tags
    }

    /// Renames a tag. A new spelling of the same name is fine; the name of another tag is not.
    pub fn tag_rename(&mut self, id: TagId, name: &str) -> Result<Tag, AppError> {
        let name_clean = Tag::normalize(name)?;
        let key = Tag::key(name)?;
        let tx = self.conn.transaction().map_err(db_error)?;
        get(&tx, id)?;
        let other: Option<String> = tx
            .query_row(
                "SELECT id FROM tag WHERE name_key = ?1 AND id <> ?2",
                params![key, id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        if other.is_some() {
            return Err(duplicate(&name_clean));
        }
        tx.execute(
            "UPDATE tag SET name = ?2, name_key = ?3 WHERE id = ?1",
            params![id.to_string(), name_clean, key],
        )
        .map_err(db_error)?;
        for frame in fts::frames_with_tag(&tx, id)? {
            fts::refresh_frame(&tx, frame)?;
        }
        let tag = get(&tx, id)?;
        tx.commit().map_err(db_error)?;
        Ok(tag)
    }

    /// Deletes a tag from the library, removing it from every frame and roll.
    pub fn tag_delete(&mut self, id: TagId) -> Result<(), AppError> {
        let tx = self.conn.transaction().map_err(db_error)?;
        let affected = fts::frames_with_tag(&tx, id)?;
        let deleted = tx
            .execute("DELETE FROM tag WHERE id = ?1", [id.to_string()])
            .map_err(db_error)?;
        if deleted == 0 {
            return Err(not_found("tag", id));
        }
        for frame in affected {
            fts::refresh_frame(&tx, frame)?;
        }
        tx.commit().map_err(db_error)
    }

    /// Adds and removes tags on several frames at once (adding twice is harmless).
    pub fn tag_frames(
        &mut self,
        frames: &[FrameId],
        add: &[TagId],
        remove: &[TagId],
    ) -> Result<(), AppError> {
        let tx = self.conn.transaction().map_err(db_error)?;
        for tag in add.iter().chain(remove) {
            get(&tx, *tag)?;
        }
        for frame in frames {
            for tag in add {
                tx.execute(
                    "INSERT OR IGNORE INTO frame_tag(frame_id, tag_id) VALUES (?1, ?2)",
                    params![frame.to_string(), tag.to_string()],
                )
                .map_err(|_| not_found("frame", frame))?;
            }
            for tag in remove {
                tx.execute(
                    "DELETE FROM frame_tag WHERE frame_id = ?1 AND tag_id = ?2",
                    params![frame.to_string(), tag.to_string()],
                )
                .map_err(db_error)?;
            }
            fts::refresh_frame(&tx, *frame)?;
        }
        tx.commit().map_err(db_error)
    }

    /// Adds and removes tags on a roll; they count for all its frames.
    pub fn tag_roll(
        &mut self,
        roll: RollId,
        add: &[TagId],
        remove: &[TagId],
    ) -> Result<(), AppError> {
        let tx = self.conn.transaction().map_err(db_error)?;
        for tag in add.iter().chain(remove) {
            get(&tx, *tag)?;
        }
        for tag in add {
            tx.execute(
                "INSERT OR IGNORE INTO roll_tag(roll_id, tag_id) VALUES (?1, ?2)",
                params![roll.to_string(), tag.to_string()],
            )
            .map_err(|_| not_found("roll", roll))?;
        }
        for tag in remove {
            tx.execute(
                "DELETE FROM roll_tag WHERE roll_id = ?1 AND tag_id = ?2",
                params![roll.to_string(), tag.to_string()],
            )
            .map_err(db_error)?;
        }
        fts::refresh_roll(&tx, roll)?;
        tx.commit().map_err(db_error)
    }

    pub fn roll_tags(&self, roll: RollId) -> Result<Vec<Tag>, AppError> {
        tags(
            &self.conn,
            "SELECT t.id, t.name FROM tag t JOIN roll_tag rt ON rt.tag_id = t.id
             WHERE rt.roll_id = ?1 ORDER BY t.name_key",
            roll.to_string(),
        )
    }

    pub fn frame_tags(&self, frame: FrameId) -> Result<FrameTags, AppError> {
        let own = tags(
            &self.conn,
            "SELECT t.id, t.name FROM tag t JOIN frame_tag ft ON ft.tag_id = t.id
             WHERE ft.frame_id = ?1 ORDER BY t.name_key",
            frame.to_string(),
        )?;
        let from_roll = tags(
            &self.conn,
            "SELECT t.id, t.name FROM tag t JOIN roll_tag rt ON rt.tag_id = t.id
             JOIN frame f ON f.roll_id = rt.roll_id WHERE f.id = ?1 ORDER BY t.name_key",
            frame.to_string(),
        )?;
        Ok(FrameTags { own, from_roll })
    }
}
