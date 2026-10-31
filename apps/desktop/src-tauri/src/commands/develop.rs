//! Develop commands over the open library (thin: parse, call the service).

use std::sync::Arc;

use gtp_app::services::develop::{self, DevelopAnalysis, DevelopInfo, LevelsPoint};
use gtp_app::AppContext;
use gtp_core::recipe::{Color, Crop, Invert};
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

/// Finds the frame, film base and levels of a frame, and the recipe they make. With `base` (three
/// linear scan values above 0, such as one picked on the rebate) the levels are fitted to that base
/// instead of the roll's or a measured one.
#[tauri::command(async)]
#[specta::specta]
pub fn develop_analyze(
    ctx: State<'_, AppContext>,
    frame_id: FrameId,
    base: Option<[f32; 3]>,
) -> CmdResult<DevelopAnalysis> {
    ctx.develop().analyze(&*ctx.library()?, frame_id, base)
}

/// The film base from an area of the scan: the per-channel median, in linear scan values.
/// `region` is in fractions of the whole scan (x, y, width, height, each 0 to 1), as a click on the
/// rebate, or a box dragged over it, gives when the picture shows the whole scan.
#[tauri::command(async)]
#[specta::specta]
pub fn develop_sample_base(
    ctx: State<'_, AppContext>,
    frame_id: FrameId,
    region: Crop,
) -> CmdResult<[f32; 3]> {
    ctx.develop()
        .sample_base(&*ctx.library()?, frame_id, region)
}

/// The recipe's levels with the white or black point set from an area of the frame, for the
/// pickers: the density of the area's median, per channel. `region` is in fractions of the cropped
/// frame as shown without straighten, turns or flips; the base and crop are the recipe's.
#[tauri::command(async)]
#[specta::specta]
pub fn develop_sample_levels(
    ctx: State<'_, AppContext>,
    frame_id: FrameId,
    recipe: Recipe,
    region: Crop,
    point: LevelsPoint,
) -> CmdResult<Invert> {
    ctx.develop()
        .sample_levels(&*ctx.library()?, frame_id, &recipe, region, point)
}

/// The recipe's colour settings with the three colour axes set so that an area of the frame becomes
/// neutral, for the neutral picker. `region` is in fractions of the cropped frame as shown without
/// straighten, turns or flips; the area is measured after the Invert stage with the recipe's base,
/// crop and levels. The other colour settings are kept.
#[tauri::command(async)]
#[specta::specta]
pub fn develop_sample_neutral(
    ctx: State<'_, AppContext>,
    frame_id: FrameId,
    recipe: Recipe,
    region: Crop,
) -> CmdResult<Color> {
    ctx.develop()
        .sample_neutral(&*ctx.library()?, frame_id, &recipe, region)
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
    // The thumbnail follows in the background; this does not wait for it.
    ctx.regenerate_thumbnails(Arc::clone(&events.0), vec![frame_id])?;
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
    let targets = frame_ids.into_iter().filter(|id| *id != source).collect();
    ctx.regenerate_thumbnails(Arc::clone(&events.0), targets)?;
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
    fn analysing_and_sampling_need_an_open_library_and_a_known_frame() {
        let dir = std::env::temp_dir().join(format!(
            "gtp-cmd-sample-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let app = tauri::test::mock_app();
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        app.manage(ctx);
        let region = Crop {
            x: 0.1,
            y: 0.1,
            w: 0.2,
            h: 0.2,
        };

        // No library open: a typed NotFound.
        let err = develop_analyze(app.state(), FrameId::new(), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        let err = develop_sample_base(app.state(), FrameId::new(), region).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);

        // A library, but no such frame.
        app.state::<AppContext>()
            .create_library(&dir.join("lib"), "Cmd")
            .unwrap();
        let err = develop_analyze(app.state(), FrameId::new(), Some([0.8, 0.5, 0.3])).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        let err = develop_sample_base(app.state(), FrameId::new(), region).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        let err = develop_sample_levels(
            app.state(),
            FrameId::new(),
            Recipe::default(),
            region,
            LevelsPoint::White,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        let err = develop_sample_neutral(app.state(), FrameId::new(), Recipe::default(), region)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }
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
