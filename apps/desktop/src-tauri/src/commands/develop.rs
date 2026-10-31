//! Develop commands over the open library (thin: parse, call the service).

use gtp_app::services::develop::{self, DevelopAnalysis, DevelopInfo};
use gtp_app::AppContext;
use gtp_core::{FrameId, Recipe, RecipeField};
use tauri::State;

use crate::error::CmdResult;
use crate::sink::EventsState;

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

/// Saves a frame's recipe, clamped into range, and marks the frame edited. Returns the recipe as
/// stored. A recipe that could not be rendered (a tone curve with a single point) is refused.
#[tauri::command(async)]
#[specta::specta]
pub fn recipe_save(
    ctx: State<'_, AppContext>,
    events: State<'_, EventsState>,
    frame_id: FrameId,
    recipe: Recipe,
) -> CmdResult<Recipe> {
    let stored = develop::save_recipe(&*ctx.library()?, frame_id, recipe)?;
    events.0.library_changed(None);
    Ok(stored)
}

/// Copies the chosen groups of settings from the source frame's recipe onto the frames, all or
/// nothing. Whatever else a target has is kept.
#[tauri::command(async)]
#[specta::specta]
pub fn recipe_apply_to(
    ctx: State<'_, AppContext>,
    events: State<'_, EventsState>,
    frame_ids: Vec<FrameId>,
    source: FrameId,
    fields: Vec<RecipeField>,
) -> CmdResult<()> {
    develop::apply_recipe_to(&*ctx.library()?, &frame_ids, source, &fields)?;
    events.0.library_changed(None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use gtp_app::services::roll_frame::{roll_create, RollInput};
    use gtp_core::{ErrorCode, Frame, FrameKind};
    use tauri::Manager;

    use super::*;
    use crate::sink::TauriJobSink;

    #[test]
    fn recipe_commands_map_errors_and_run_against_the_open_library() {
        let dir = std::env::temp_dir().join(format!(
            "gtp-cmd-recipe-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let app = tauri::test::mock_app();
        crate::ipc::builder::<tauri::test::MockRuntime>().mount_events(&app);
        app.manage(EventsState(std::sync::Arc::new(TauriJobSink::new(
            app.handle().clone(),
        ))));
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        app.manage(ctx);

        // No library open: a typed NotFound.
        let err =
            recipe_save(app.state(), app.state(), FrameId::new(), Recipe::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);

        let ctx = app.state::<AppContext>();
        ctx.create_library(&dir.join("lib"), "Cmd").unwrap();
        let library = ctx.library().unwrap();
        let roll = roll_create(
            &library,
            RollInput {
                name: "Roll".into(),
                ..RollInput::default()
            },
        )
        .unwrap();
        let frames: Vec<FrameId> = (1..=2)
            .map(|index| {
                library
                    .write(|c| {
                        c.insert_frame(&Frame {
                            id: FrameId::new(),
                            roll_id: roll.id,
                            index_in_roll: index,
                            kind: FrameKind::ScanRgbIr,
                            width: 10,
                            height: 10,
                            has_ir: false,
                            note: String::new(),
                            created_at: String::new(),
                            edited_at: None,
                        })
                    })
                    .unwrap()
                    .id
            })
            .collect();

        let mut recipe = Recipe::default();
        recipe.tone.exposure_ev = 5000.0;
        let stored = recipe_save(app.state(), app.state(), frames[0], recipe).unwrap();
        assert_ne!(stored.tone.exposure_ev, 5000.0, "clamped");

        recipe_apply_to(
            app.state(),
            app.state(),
            vec![frames[1]],
            frames[0],
            vec![RecipeField::Tone],
        )
        .unwrap();
        let copied = library.read(|c| c.load_recipe(frames[1])).unwrap().unwrap();
        assert_eq!(copied.tone, stored.tone);

        let err = recipe_apply_to(app.state(), app.state(), vec![frames[1]], frames[0], vec![])
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "InvalidInput");

        drop(library);
        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
