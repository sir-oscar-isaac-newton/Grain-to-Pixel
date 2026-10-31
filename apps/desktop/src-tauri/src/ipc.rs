//! The typed IPC surface: every command and event registered here ends up in the generated
//! `packages/ipc-types/src/bindings.ts`. Run `pnpm gen:ipc` after changing it.

use std::path::Path;

use specta_typescript::Typescript;
use tauri_specta::{collect_commands, collect_events, Builder};

use crate::commands;
use crate::events::{JobDone, JobError, JobProgress, LibraryChanged};

/// Collects all commands and events (one place, shared by the app, the generator and tests on
/// the mock runtime).
pub fn builder<R: tauri::Runtime>() -> Builder<R> {
    Builder::<R>::new()
        .commands(collect_commands![
            commands::library::library_info,
            commands::library::library_create,
            commands::library::library_open,
            commands::library::library_verify,
            commands::library::folder_check,
            commands::roll::roll_create,
            commands::roll::roll_update,
            commands::roll::roll_list,
            commands::roll::roll_set_film_base,
            commands::roll::roll_delete,
            commands::frame::frame_list,
            commands::frame::frame_get,
            commands::frame::frame_set_note,
            commands::frame::frame_delete,
            commands::ingest::import_files,
            commands::export::export_start,
            commands::develop::develop_open,
            commands::develop::develop_analyze,
            commands::develop::develop_sample_base,
            commands::develop::develop_sample_levels,
            commands::develop::develop_sample_neutral,
            commands::develop::recipe_save,
            commands::develop::recipe_apply_to,
            commands::tags::tag_list,
            commands::tags::roll_tags,
            commands::tags::frames_own_tags,
            commands::tags::tag_apply,
            commands::tags::tag_apply_roll,
            commands::tags::tag_rename,
            commands::tags::tag_delete,
            commands::search::search,
            commands::system::settings_get,
            commands::system::settings_set,
            commands::system::log_client_event,
            commands::system::crash_notes_take,
            commands::system::job_cancel
        ])
        .events(collect_events![
            JobProgress,
            JobDone,
            JobError,
            LibraryChanged
        ])
}

/// Commands whose result is raw bytes (`tauri::ipc::Response`), which specta cannot describe. They
/// are left out of the typed bindings and called by name from `shared/ipc.ts`.
const RAW_COMMANDS: [&str; 2] = ["frame_thumbnail", "develop_render"];

/// The handler of the raw-byte commands (a function of its own, so the closure's type is known).
fn raw_handler<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static
{
    tauri::generate_handler![
        commands::frame::frame_thumbnail,
        commands::develop::develop_render
    ]
}

/// The handler of the whole app: the typed commands of [`builder`] plus the raw-byte ones.
pub fn invoke_handler<R: tauri::Runtime>(
    builder: &Builder<R>,
) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static + use<'_, R> {
    let typed = builder.invoke_handler();
    let raw = raw_handler::<R>();
    move |invoke| {
        if RAW_COMMANDS.contains(&invoke.message.command()) {
            raw(invoke)
        } else {
            typed(invoke)
        }
    }
}

/// Writes the TypeScript bindings to `path` without starting the app.
pub fn export_bindings(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    builder::<tauri::Wry>().export(Typescript::default(), path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_describe_commands_and_the_error_type() {
        let path = std::env::temp_dir().join(format!("gtp-bindings-{}.ts", std::process::id()));
        export_bindings(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert!(
            text.contains("libraryInfo"),
            "command missing from bindings"
        );
        for command in [
            "logClientEvent",
            "crashNotesTake",
            "jobCancel",
            "libraryCreate",
            "libraryOpen",
            "libraryVerify",
            "folderCheck",
            "developOpen",
            "developAnalyze",
            "developSampleBase",
            "developSampleLevels",
            "developSampleNeutral",
            "recipeSave",
            "recipeApplyTo",
            "settingsGet",
            "settingsSet",
            "rollCreate",
            "rollUpdate",
            "rollList",
            "rollSetFilmBase",
            "rollDelete",
            "frameList",
            "frameGet",
            "frameSetNote",
            "frameDelete",
            "importFiles",
            "exportStart",
            "tagList",
            "rollTags",
            "framesOwnTags",
            "tagApply",
            "tagApplyRoll",
            "tagRename",
            "tagDelete",
            "search",
        ] {
            assert!(text.contains(command), "{command} missing from bindings");
        }
        for event in [
            "job://progress",
            "job://done",
            "job://error",
            "library://changed",
        ] {
            assert!(text.contains(event), "event {event} missing from bindings");
        }
        for ty in [
            "JobProgress",
            "JobDone",
            "JobError",
            "JobResult",
            "RollInput",
            "FramePage",
            "FrameSummary",
            "DeleteReport",
            "ImportResult",
            "SkipReason",
            "ImportKind",
            "SearchQuery",
            "Tag",
            "VerifyResult",
            "VerifyProblem",
            "VerifyReason",
            "FolderCheck",
            "DevelopInfo",
            "DevelopAnalysis",
            "RecipeField",
        ] {
            assert!(text.contains(ty), "type {ty} missing from bindings");
        }
        assert!(text.contains("AppError"), "AppError missing from bindings");
        for code in gtp_core::ErrorCode::ALL {
            assert!(
                text.contains(&format!("{code:?}")),
                "ErrorCode::{code:?} missing"
            );
        }
    }

    #[test]
    fn export_is_deterministic() {
        let dir = std::env::temp_dir();
        let a = dir.join(format!("gtp-bindings-a-{}.ts", std::process::id()));
        let b = dir.join(format!("gtp-bindings-b-{}.ts", std::process::id()));
        export_bindings(&a).unwrap();
        export_bindings(&b).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
        std::fs::remove_file(&a).unwrap();
        std::fs::remove_file(&b).unwrap();
    }

    /// Sends one command through the app's real handler on the mock runtime.
    fn call(
        webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        cmd: &str,
        body: serde_json::Value,
    ) -> Result<tauri::ipc::InvokeResponseBody, serde_json::Value> {
        tauri::test::get_ipc_response(
            webview,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                // The app's own origin differs by platform; any other counts as remote.
                url: if cfg!(windows) {
                    "http://tauri.localhost"
                } else {
                    "tauri://localhost"
                }
                .parse()
                .unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
    }

    #[test]
    fn a_thumbnail_travels_as_raw_bytes_and_typed_commands_still_route() {
        use gtp_app::services::roll_frame::{roll_create, RollInput};
        use gtp_app::AppContext;
        use gtp_core::{Frame, FrameId, FrameKind};
        use tauri::ipc::InvokeResponseBody;

        let dir = std::env::temp_dir().join(format!(
            "gtp-ipc-raw-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        let details = ctx.create_library(&dir.join("lib"), "Raw").unwrap();
        let library = ctx.library().unwrap();
        let roll = roll_create(
            &library,
            RollInput {
                name: "R".into(),
                ..RollInput::default()
            },
        )
        .unwrap();
        let frame = library
            .write(|c| {
                c.insert_frame(&Frame {
                    id: FrameId::new(),
                    roll_id: roll.id,
                    index_in_roll: 1,
                    kind: FrameKind::ScanRgbIr,
                    width: 10,
                    height: 10,
                    has_ir: false,
                    note: String::new(),
                    created_at: String::new(),
                    edited_at: None,
                })
            })
            .unwrap();
        let thumb = gtp_app::library_fs::thumb_rel(frame.id);
        std::fs::write(library.resolve(&thumb).unwrap(), [0xFF, 0xD8, 1, 2, 3]).unwrap();

        let typed = builder::<tauri::test::MockRuntime>();
        let app = tauri::test::mock_builder()
            .invoke_handler(invoke_handler(&typed))
            .setup(move |app| {
                typed.mount_events(app);
                Ok(())
            })
            .manage(ctx)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();

        // The thumbnail is the file's bytes, not JSON.
        match call(
            &webview,
            "frame_thumbnail",
            serde_json::json!({ "id": frame.id }),
        )
        .unwrap()
        {
            InvokeResponseBody::Raw(bytes) => assert_eq!(bytes, [0xFF, 0xD8, 1, 2, 3]),
            other => unreachable!("expected raw bytes, got {other:?}"),
        }
        // A frame without a thumbnail is a typed error, as for every command.
        let err = call(
            &webview,
            "frame_thumbnail",
            serde_json::json!({ "id": FrameId::new() }),
        )
        .unwrap_err();
        assert_eq!(err["code"], "NotFound");
        // A typed command takes the other route and answers in JSON.
        match call(
            &webview,
            "folder_check",
            serde_json::json!({ "path": details.path }),
        )
        .unwrap()
        {
            InvokeResponseBody::Json(json) => assert!(json.contains("\"has_library\":true")),
            other => unreachable!("expected JSON, got {other:?}"),
        }

        // The develop preview is a raw-byte command too: it is routed to the service (a frame
        // without a scan file is `NotFound`, a bad size `InvalidInput`, both typed errors), while
        // `develop_open` answers in JSON. The pixels themselves are tested in `gtp-app`.
        let recipe = serde_json::to_value(gtp_core::Recipe::default()).unwrap();
        let render = |edge: u32, frame: FrameId| {
            call(
                &webview,
                "develop_render",
                serde_json::json!({ "frameId": frame, "recipe": recipe, "maxEdge": edge, "seq": 1 }),
            )
        };
        assert_eq!(render(1600, frame.id).unwrap_err()["code"], "NotFound");
        assert_eq!(render(10, frame.id).unwrap_err()["code"], "InvalidInput");
        let opened = call(
            &webview,
            "develop_open",
            serde_json::json!({ "frameId": frame.id }),
        )
        .unwrap_err();
        assert_eq!(opened["code"], "NotFound");
        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
