//! Search command over the open library.

use gtp_app::services::roll_frame::FramePage;
use gtp_app::services::tags_search;
use gtp_app::AppContext;
use gtp_core::SearchQuery;
use tauri::State;

use crate::error::CmdResult;

/// One page of the frames matching `query`: best text match first when there is text, otherwise
/// newest roll first. `page` counts from 0, `page_size` is 1 to 500.
#[tauri::command(async)]
#[specta::specta]
pub fn search(
    ctx: State<'_, AppContext>,
    query: SearchQuery,
    page: u32,
    page_size: u32,
) -> CmdResult<FramePage> {
    tags_search::search(&*ctx.library()?, &query, page, page_size)
}

#[cfg(test)]
mod tests {
    use gtp_app::services::roll_frame::{roll_create, RollInput};
    use gtp_core::{ErrorCode, FrameId};
    use tauri::Manager;

    use super::*;
    use crate::commands::tags::{tag_apply_roll, tag_list};
    use crate::sink::{EventsState, TauriJobSink};

    #[test]
    fn search_and_tag_commands_map_errors_and_run_against_the_open_library() {
        let dir = std::env::temp_dir().join(format!(
            "gtp-cmd-search-{}-{}",
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
        let err = search(app.state(), SearchQuery::default(), 0, 10).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(tag_list(app.state()).unwrap_err().code, ErrorCode::NotFound);

        let ctx = app.state::<AppContext>();
        ctx.create_library(&dir.join("lib"), "Cmd").unwrap();
        let roll = roll_create(
            &ctx.library().unwrap(),
            RollInput {
                name: "Roll".into(),
                camera: Some("Nikon F3".into()),
                ..RollInput::default()
            },
        )
        .unwrap();
        let none = search(app.state(), SearchQuery::default(), 0, 10).unwrap();
        assert_eq!(none.total, 0, "a roll without frames has nothing to find");

        tag_apply_roll(
            app.state(),
            app.state(),
            roll.id,
            vec!["Japan".into()],
            vec![],
        )
        .unwrap();
        assert_eq!(tag_list(app.state()).unwrap().len(), 1);
        let bad = SearchQuery {
            date_from: Some("2026-13".into()),
            ..SearchQuery::default()
        };
        let err = search(app.state(), bad, 0, 10).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "InvalidInput");

        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
