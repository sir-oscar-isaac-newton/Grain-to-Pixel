//! Roll commands over the open library (thin: parse, call the service, log).

use std::sync::Arc;

use gtp_app::services::flat_field::{self, FlatFieldInfo};
use gtp_app::services::roll_frame::{self, DeleteReport, RollInput};
use gtp_app::AppContext;
use gtp_core::{Roll, RollId};
use tauri::State;

use crate::error::CmdResult;
use crate::sink::EventsState;

/// Logs files a delete could not remove (the UI also gets them in the report).
pub(crate) fn log_files_left(report: &DeleteReport) {
    if !report.files_left.is_empty() {
        tracing::warn!(files = ?report.files_left, "files left after delete");
    }
}

#[tauri::command(async)]
#[specta::specta]
pub fn roll_create(ctx: State<'_, AppContext>, input: RollInput) -> CmdResult<Roll> {
    roll_frame::roll_create(&*ctx.library()?, input)
}

/// Replaces every editable field of the roll with `input` (an empty field is cleared).
#[tauri::command(async)]
#[specta::specta]
pub fn roll_update(ctx: State<'_, AppContext>, id: RollId, input: RollInput) -> CmdResult<Roll> {
    roll_frame::roll_update(&*ctx.library()?, id, input)
}

/// All rolls, newest first.
#[tauri::command(async)]
#[specta::specta]
pub fn roll_list(ctx: State<'_, AppContext>) -> CmdResult<Vec<Roll>> {
    roll_frame::roll_list(&*ctx.library()?)
}

/// Sets the roll's film base (three linear values above 0), or clears it with `null`. The frames
/// that follow the roll's base look different now, so their thumbnails are regenerated in the
/// background (a frame with a base picked by hand does not change).
#[tauri::command(async)]
#[specta::specta]
pub fn roll_set_film_base(
    ctx: State<'_, AppContext>,
    events: State<'_, EventsState>,
    id: RollId,
    base: Option<[f32; 3]>,
) -> CmdResult<Roll> {
    let library = ctx.library()?;
    let roll = roll_frame::roll_set_film_base(&library, id, base)?;
    let following = roll_frame::frames_following_roll_base(&library, id)?;
    ctx.regenerate_thumbnails(Arc::clone(&events.0), following)?;
    Ok(roll)
}

/// Sets the size of one frame on the roll's captures by hand (width and height as fractions of the
/// image), or clears it with `null` so it is measured again. Frames that were never edited get a new
/// automatic crop, so their thumbnails are regenerated in the background.
#[tauri::command(async)]
#[specta::specta]
pub fn roll_set_frame_size(
    ctx: State<'_, AppContext>,
    events: State<'_, EventsState>,
    id: RollId,
    size: Option<[f32; 2]>,
) -> CmdResult<Roll> {
    let library = ctx.library()?;
    let roll = roll_frame::roll_set_frame_size(&library, id, size)?;
    let following = roll_frame::frames_following_roll_base(&library, id)?;
    ctx.regenerate_thumbnails(Arc::clone(&events.0), following)?;
    Ok(roll)
}

/// What changes when a roll's flat field is attached or removed: the proxies Develop kept were built
/// without it, an automatic film base was measured without it, and every frame of the roll looks
/// different, so all their thumbnails are made again in the background.
fn after_flat_field_change(
    ctx: &AppContext,
    events: &EventsState,
    id: RollId,
) -> Result<(), gtp_core::AppError> {
    let library = ctx.library()?;
    ctx.develop().clear();
    flat_field::remeasure_roll_base(&library, id)?;
    let frames = library
        .read(|c| c.list_frames(id))?
        .into_iter()
        .map(|f| f.id)
        .collect();
    ctx.regenerate_thumbnails(Arc::clone(&events.0), frames)
}

/// Makes the roll's flat field from three bare-light exposures (red, green and blue, in any order:
/// they are told apart by colour) and attaches it, replacing the one it has. The roll's camera-triplet
/// frames are divided by it from now on. The three files must fit the roll's frames in size.
#[tauri::command(async)]
#[specta::specta]
pub fn flat_field_import(
    ctx: State<'_, AppContext>,
    events: State<'_, EventsState>,
    id: RollId,
    paths: Vec<String>,
) -> CmdResult<FlatFieldInfo> {
    let paths: Vec<std::path::PathBuf> = paths.into_iter().map(Into::into).collect();
    let info = flat_field::flat_field_import(&*ctx.library()?, id, &paths)?;
    after_flat_field_change(&ctx, &events, id)?;
    tracing::info!(roll = %id, "flat field attached");
    Ok(info)
}

/// Removes the roll's flat field; true when it had one.
#[tauri::command(async)]
#[specta::specta]
pub fn flat_field_clear(
    ctx: State<'_, AppContext>,
    events: State<'_, EventsState>,
    id: RollId,
) -> CmdResult<bool> {
    let removed = flat_field::flat_field_clear(&*ctx.library()?, id)?;
    if removed {
        after_flat_field_change(&ctx, &events, id)?;
        tracing::info!(roll = %id, "flat field removed");
    }
    Ok(removed)
}

/// Deletes a roll with its frames, then their files.
#[tauri::command(async)]
#[specta::specta]
pub fn roll_delete(ctx: State<'_, AppContext>, id: RollId) -> CmdResult<DeleteReport> {
    let report = roll_frame::roll_delete(&*ctx.library()?, id)?;
    tracing::info!(roll = %id, frames = report.frames_deleted, "roll deleted");
    log_files_left(&report);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use gtp_app::services::roll_frame::FrameQuery;
    use gtp_core::{ErrorCode, FrameId};
    use tauri::Manager;

    use super::*;
    use crate::commands::frame::{frame_delete, frame_list};

    #[test]
    fn commands_map_errors_and_run_against_the_open_library() {
        let dir = std::env::temp_dir().join(format!(
            "gtp-cmd-roll-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let app = tauri::test::mock_app();
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        app.manage(ctx);

        // No library open: a typed NotFound that reaches the UI as { code, message }.
        let err = roll_list(app.state()).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "NotFound");
        assert!(json["message"].as_str().is_some_and(|m| !m.is_empty()));

        app.state::<AppContext>()
            .create_library(&dir.join("lib"), "Cmd")
            .unwrap();
        let input = RollInput {
            name: "Roll 1".into(),
            camera: Some("Nikon F3".into()),
            ..RollInput::default()
        };
        let roll = roll_create(app.state(), input).unwrap();
        assert_eq!(roll_list(app.state()).unwrap(), vec![roll.clone()]);

        let bad = RollInput {
            iso_box: Some(0),
            ..RollInput::default()
        };
        assert_eq!(
            roll_create(app.state(), bad).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let page = frame_list(
            app.state(),
            FrameQuery {
                roll_id: Some(roll.id),
                flag: None,
                needs_export: false,
                page: 0,
                page_size: 50,
            },
        )
        .unwrap();
        assert_eq!(page.total, 0);
        assert_eq!(frame_delete(app.state(), vec![]).unwrap().frames_deleted, 0);
        assert_eq!(roll_delete(app.state(), roll.id).unwrap().frames_deleted, 0);

        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rolls_base_is_set_checked_and_cleared() {
        let dir = std::env::temp_dir().join(format!(
            "gtp-cmd-rollbase-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let app = tauri::test::mock_app();
        crate::ipc::builder::<tauri::test::MockRuntime>().mount_events(&app);
        app.manage(EventsState(Arc::new(crate::sink::TauriJobSink::new(
            app.handle().clone(),
        ))));
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        app.manage(ctx);

        let err = roll_set_film_base(app.state(), app.state(), RollId::new(), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "no library is open");

        app.state::<AppContext>()
            .create_library(&dir.join("lib"), "Cmd")
            .unwrap();
        let input = RollInput {
            name: "Roll".into(),
            ..RollInput::default()
        };
        let roll = roll_create(app.state(), input).unwrap();

        let base = Some([0.8, 0.5, 0.3]);
        let set = roll_set_film_base(app.state(), app.state(), roll.id, base).unwrap();
        assert_eq!(set.film_base_rgb, base);
        let bad = roll_set_film_base(app.state(), app.state(), roll.id, Some([0.8, 0.0, 0.3]));
        assert_eq!(bad.unwrap_err().code, ErrorCode::InvalidInput);
        let cleared = roll_set_film_base(app.state(), app.state(), roll.id, None).unwrap();
        assert_eq!(cleared.film_base_rgb, None);

        // The frame size is set by hand, checked, and cleared; an update keeps it.
        let sized = roll_set_frame_size(app.state(), app.state(), roll.id, Some([0.6, 0.9]));
        let sized = sized.unwrap().frame_size.unwrap();
        assert_eq!((sized.w, sized.h, sized.samples), (0.6, 0.9, 0));
        let bad = roll_set_frame_size(app.state(), app.state(), roll.id, Some([0.0, 0.9]));
        assert_eq!(bad.unwrap_err().code, ErrorCode::InvalidInput);
        let input = RollInput {
            name: "Renamed".into(),
            format: gtp_core::FilmFormat::Mf645,
            ..RollInput::default()
        };
        let updated = roll_update(app.state(), roll.id, input).unwrap();
        assert_eq!(updated.format, gtp_core::FilmFormat::Mf645);
        assert!(updated.frame_size.is_some());
        let cleared = roll_set_frame_size(app.state(), app.state(), roll.id, None).unwrap();
        assert_eq!(cleared.frame_size, None);

        // A flat field is three files of one capture; nothing is attached by a refusal, and a roll
        // with none says so when it is told to drop it.
        let two = vec!["a.NEF".to_string(), "b.NEF".to_string()];
        let err = flat_field_import(app.state(), app.state(), roll.id, two).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(!flat_field_clear(app.state(), app.state(), roll.id).unwrap());
        let err = flat_field_clear(app.state(), app.state(), RollId::new()).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);

        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
