//! Keeps the `search` table in step with rolls, frames and tags. Every function runs on the caller's
//! transaction, so a search row never disagrees with committed data. The FTS row of a frame uses
//! the frame's rowid, so it can be replaced without scanning.

use rusqlite::{params, Connection};

use gtp_core::{AppError, FrameId, RollId, TagId};

use crate::db::db_error;

/// The text of each frame selected by the trailing `WHERE`: roll name, film, camera, ISO, dates and
/// notes, the frame note, and the names of the frame's own and its roll's tags.
const INSERT_TEXT: &str = "
    INSERT INTO search(rowid, frame_id, text)
    SELECT f.rowid, f.id, concat_ws(' ',
        r.name, r.film_stock, r.camera, r.iso_box, r.iso_shot, r.shot_from, r.shot_to, r.notes,
        f.note,
        (SELECT group_concat(t.name, ' ') FROM tag t WHERE t.id IN (
            SELECT tag_id FROM frame_tag WHERE frame_id = f.id
            UNION SELECT tag_id FROM roll_tag WHERE roll_id = f.roll_id)))
    FROM frame f JOIN roll r ON r.id = f.roll_id";

/// Replaces every search row with a fresh one.
pub(crate) fn rebuild_all(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM search", []).map_err(db_error)?;
    conn.execute(INSERT_TEXT, []).map_err(db_error)?;
    Ok(())
}

pub(crate) fn refresh_frame(conn: &Connection, id: FrameId) -> Result<(), AppError> {
    let id = id.to_string();
    conn.execute(
        "DELETE FROM search WHERE rowid = (SELECT rowid FROM frame WHERE id = ?1)",
        [&id],
    )
    .map_err(db_error)?;
    conn.execute(&format!("{INSERT_TEXT} WHERE f.id = ?1"), [&id])
        .map_err(db_error)?;
    Ok(())
}

pub(crate) fn refresh_roll(conn: &Connection, id: RollId) -> Result<(), AppError> {
    let id = id.to_string();
    conn.execute(
        "DELETE FROM search WHERE rowid IN (SELECT rowid FROM frame WHERE roll_id = ?1)",
        [&id],
    )
    .map_err(db_error)?;
    conn.execute(&format!("{INSERT_TEXT} WHERE f.roll_id = ?1"), [&id])
        .map_err(db_error)?;
    Ok(())
}

/// Removes the search rows of frames that are about to be deleted (call before the delete).
pub(crate) fn forget_frame(conn: &Connection, id: FrameId) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM search WHERE rowid = (SELECT rowid FROM frame WHERE id = ?1)",
        [id.to_string()],
    )
    .map(|_| ())
    .map_err(db_error)
}

pub(crate) fn forget_roll(conn: &Connection, id: RollId) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM search WHERE rowid IN (SELECT rowid FROM frame WHERE roll_id = ?1)",
        [id.to_string()],
    )
    .map(|_| ())
    .map_err(db_error)
}

/// The frames whose text includes `tag`: tagged themselves or on a tagged roll.
pub(crate) fn frames_with_tag(conn: &Connection, tag: TagId) -> Result<Vec<FrameId>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT frame_id FROM frame_tag WHERE tag_id = ?1
             UNION SELECT f.id FROM frame f JOIN roll_tag rt ON rt.roll_id = f.roll_id
             WHERE rt.tag_id = ?1",
        )
        .map_err(db_error)?;
    let ids = stmt
        .query_map(params![tag.to_string()], |row| row.get::<_, String>(0))
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    ids.iter().map(|id| id.parse()).collect()
}
