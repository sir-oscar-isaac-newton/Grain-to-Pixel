//! Frame commands over the open library (thin: parse, call the service, log).

use gtp_app::services::roll_frame::FrameSummary;
use gtp_app::services::roll_frame::{self, DeleteReport, FramePage, FrameQuery};
use gtp_app::AppContext;
use gtp_core::{Frame, FrameFlag, FrameId};
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

/// Sets the pick or reject mark of frames (unknown ids are skipped) and returns the changed frames.
#[tauri::command(async)]
#[specta::specta]
pub fn frame_set_flag(
    ctx: State<'_, AppContext>,
    ids: Vec<FrameId>,
    flag: FrameFlag,
) -> CmdResult<Vec<FrameSummary>> {
    let frames = roll_frame::frame_set_flag(&*ctx.library()?, &ids, flag)?;
    tracing::info!(
        frames = frames.len(),
        flag = flag.as_str(),
        "frames flagged"
    );
    Ok(frames)
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

#[cfg(test)]
mod tests {
    use gtp_app::services::roll_frame::{roll_create, RollInput};
    use gtp_core::{ErrorCode, FrameKind};
    use tauri::Manager;

    use super::*;

    #[test]
    fn flagging_runs_against_the_open_library_and_filters_the_list() {
        let dir = std::env::temp_dir().join(format!(
            "gtp-cmd-flag-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let app = tauri::test::mock_app();
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        app.manage(ctx);

        // No library open: a typed NotFound.
        let err = frame_set_flag(app.state(), vec![], FrameFlag::Pick).unwrap_err();
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
                            flag: FrameFlag::None,
                            created_at: String::new(),
                            edited_at: None,
                        })
                    })
                    .unwrap()
                    .id
            })
            .collect();

        let changed = frame_set_flag(
            app.state(),
            vec![frames[1], FrameId::new()],
            FrameFlag::Reject,
        )
        .unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].id, frames[1]);
        assert_eq!(changed[0].flag, FrameFlag::Reject);
        let json = serde_json::to_value(&changed[0]).unwrap();
        assert_eq!(json["flag"], "reject");

        let page = frame_list(
            app.state(),
            FrameQuery {
                roll_id: Some(roll.id),
                flag: Some(FrameFlag::Reject),
                page: 0,
                page_size: 50,
            },
        )
        .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].id, frames[1]);

        drop(library);
        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
