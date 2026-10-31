//! Tag commands over the open library (thin: parse, call the service, announce the change).

use gtp_app::services::tags_search;
use gtp_app::AppContext;
use gtp_core::{FrameId, RollId, Tag, TagId};
use tauri::State;

use crate::error::CmdResult;
use crate::sink::EventsState;

/// All tags, in name order.
#[tauri::command(async)]
#[specta::specta]
pub fn tag_list(ctx: State<'_, AppContext>) -> CmdResult<Vec<Tag>> {
    tags_search::tag_list(&*ctx.library()?)
}

/// Adds the tags named in `add` (created when missing) to the frames and removes those named in
/// `remove`, all or nothing.
#[tauri::command(async)]
#[specta::specta]
pub fn tag_apply(
    ctx: State<'_, AppContext>,
    events: State<'_, EventsState>,
    frame_ids: Vec<FrameId>,
    add: Vec<String>,
    remove: Vec<String>,
) -> CmdResult<()> {
    tags_search::tag_apply(&*ctx.library()?, &frame_ids, &add, &remove)?;
    events.0.library_changed(None);
    Ok(())
}

/// The same for a roll: its tags count for every frame on it, also frames added later.
#[tauri::command(async)]
#[specta::specta]
pub fn tag_apply_roll(
    ctx: State<'_, AppContext>,
    events: State<'_, EventsState>,
    roll_id: RollId,
    add: Vec<String>,
    remove: Vec<String>,
) -> CmdResult<()> {
    tags_search::tag_apply_roll(&*ctx.library()?, roll_id, &add, &remove)?;
    events.0.library_changed(Some(roll_id));
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn tag_rename(
    ctx: State<'_, AppContext>,
    events: State<'_, EventsState>,
    id: TagId,
    name: String,
) -> CmdResult<Tag> {
    let tag = tags_search::tag_rename(&*ctx.library()?, id, &name)?;
    events.0.library_changed(None);
    Ok(tag)
}

#[tauri::command(async)]
#[specta::specta]
pub fn tag_delete(
    ctx: State<'_, AppContext>,
    events: State<'_, EventsState>,
    id: TagId,
) -> CmdResult<()> {
    tags_search::tag_delete(&*ctx.library()?, id)?;
    events.0.library_changed(None);
    Ok(())
}
