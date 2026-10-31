//! Frame commands over the open library (thin: parse, call the service, log).

use gtp_app::services::roll_frame::{self, DeleteReport, FramePage, FrameQuery};
use gtp_app::AppContext;
use gtp_core::{Frame, FrameId};
use tauri::State;

use super::roll::log_files_left;
use crate::error::CmdResult;

/// One page of frames of a roll, or of the whole library.
#[tauri::command(async)]
#[specta::specta]
pub fn frame_list(ctx: State<'_, AppContext>, query: FrameQuery) -> CmdResult<FramePage> {
    roll_frame::frame_list(&*ctx.library()?, query)
}

/// The frame's thumbnail (a JPEG) as raw bytes, not JSON: the UI turns it into an image. A missing
/// thumbnail file is rebuilt from the frame's saved recipe first; `NotFound` remains when the frame
/// or its original is gone. Not in the typed bindings (see `ipc::RAW_COMMANDS`).
#[tauri::command(async)]
pub fn frame_thumbnail(ctx: State<'_, AppContext>, id: FrameId) -> CmdResult<tauri::ipc::Response> {
    let bytes = ctx.thumbnail(id)?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command(async)]
#[specta::specta]
pub fn frame_get(ctx: State<'_, AppContext>, id: FrameId) -> CmdResult<Frame> {
    roll_frame::frame_get(&*ctx.library()?, id)
}

#[tauri::command(async)]
#[specta::specta]
pub fn frame_set_note(ctx: State<'_, AppContext>, id: FrameId, note: String) -> CmdResult<Frame> {
    roll_frame::frame_set_note(&*ctx.library()?, id, &note)
}

/// Deletes frames (unknown ids are skipped), then their originals and thumbnails.
#[tauri::command(async)]
#[specta::specta]
pub fn frame_delete(ctx: State<'_, AppContext>, ids: Vec<FrameId>) -> CmdResult<DeleteReport> {
    let report = roll_frame::frame_delete(&*ctx.library()?, &ids)?;
    tracing::info!(frames = report.frames_deleted, "frames deleted");
    log_files_left(&report);
    Ok(report)
}
