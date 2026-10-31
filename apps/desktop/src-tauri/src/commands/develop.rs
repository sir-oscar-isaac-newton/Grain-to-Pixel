//! Develop commands over the open library (thin: parse, call the service).

use gtp_app::services::develop::{DevelopAnalysis, DevelopInfo};
use gtp_app::AppContext;
use gtp_core::{FrameId, Recipe};
use tauri::State;

use crate::error::CmdResult;

/// Opens a frame for editing: decodes its original into the proxy cache and returns its size, saved
/// recipe and the roll's film base.
#[tauri::command(async)]
#[specta::specta]
pub fn develop_open(ctx: State<'_, AppContext>, frame_id: FrameId) -> CmdResult<DevelopInfo> {
    ctx.develop().open(&*ctx.library()?, frame_id)
}

/// Finds the frame, film base and levels of a frame, and the recipe they make.
#[tauri::command(async)]
#[specta::specta]
pub fn develop_analyze(
    ctx: State<'_, AppContext>,
    frame_id: FrameId,
) -> CmdResult<DevelopAnalysis> {
    ctx.develop().analyze(&*ctx.library()?, frame_id)
}

/// A preview of the frame with `recipe`, as raw bytes, not JSON: a 16-byte little-endian header
/// (`seq` u64, `width` u32, `height` u32) then RGBA8 sRGB pixels. A request superseded by a newer
/// `seq` for the same frame is `Cancelled`. Not in the typed bindings (see `ipc::RAW_COMMANDS`).
#[tauri::command(async)]
pub fn develop_render(
    ctx: State<'_, AppContext>,
    frame_id: FrameId,
    recipe: Recipe,
    max_edge: u32,
    seq: u64,
) -> CmdResult<tauri::ipc::Response> {
    let bytes = ctx
        .develop()
        .render(&*ctx.library()?, frame_id, &recipe, max_edge, seq)?;
    Ok(tauri::ipc::Response::new(bytes))
}
