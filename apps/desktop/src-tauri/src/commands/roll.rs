//! Roll commands over the open library (thin: parse, call the service, log).

use gtp_app::services::roll_frame::{self, DeleteReport, RollInput};
use gtp_app::AppContext;
use gtp_core::{Roll, RollId};
use tauri::State;

use crate::error::CmdResult;

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

/// Sets the roll's film base (three linear values above 0), or clears it with `null`.
#[tauri::command(async)]
#[specta::specta]
pub fn roll_set_film_base(
    ctx: State<'_, AppContext>,
    id: RollId,
    base: Option<[f32; 3]>,
) -> CmdResult<Roll> {
    roll_frame::roll_set_film_base(&*ctx.library()?, id, base)
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
}
