//! Update commands: ask GitHub Releases whether a newer version exists, and install it when the
//! user says so (specification sections 9 and 15).
//!
//! The check is the only network call the app makes. It happens at launch at most once a day, and
//! never when the `update_check` flag is off. Nothing is installed without a click, and a package
//! whose signature does not match the public key in `tauri.conf.json` is never installed.

use std::sync::PoisonError;
use std::time::{SystemTime, UNIX_EPOCH};

use gtp_app::{AppContext, JobManager, SettingsStore};
use gtp_core::{AppError, ErrorCode};
use serde::Serialize;
use specta::Type;
use tauri::State;
use tauri_plugin_updater::{Error as UpdaterError, Update, Updater};

use crate::error::CmdResult;
use crate::state::AppState;

/// What the update check found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpdateOutcome {
    /// The `update_check` flag is off: nothing was asked.
    Disabled,
    /// The last check was less than a day ago: nothing was asked.
    NotDue,
    /// This is the newest version.
    UpToDate,
    /// A newer version can be installed.
    Available {
        version: String,
        current_version: String,
        notes: Option<String>,
    },
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Turns an updater error into the error the UI gets: a server that cannot be reached, a package
/// that cannot be trusted, or something unexpected.
fn update_error(err: UpdaterError) -> AppError {
    match err {
        UpdaterError::Network(_)
        | UpdaterError::Reqwest(_)
        | UpdaterError::ReleaseNotFound
        | UpdaterError::Io(_) => AppError::new(ErrorCode::Io, "Could not reach the update server.")
            .with_details(err.to_string()),
        UpdaterError::Minisign(_)
        | UpdaterError::Base64(_)
        | UpdaterError::SignatureUtf8(_)
        | UpdaterError::SignedVersionMismatch { .. }
        | UpdaterError::MissingSignedVersion => AppError::new(
            ErrorCode::InvalidInput,
            "The update could not be verified, so it was not installed.",
        )
        .with_details(err.to_string()),
        other => AppError::internal(other),
    }
}

/// The updater built at start from the settings in `tauri.conf.json`. It is none when that failed
/// (the reason is in the log); the update commands then report an error and the app runs on.
pub struct UpdateState(pub Option<Updater>);

fn updater_of(state: &UpdateState) -> CmdResult<&Updater> {
    state
        .0
        .as_ref()
        .ok_or_else(|| AppError::internal("the updater could not be set up; see the log"))
}

/// Asks the update server, unless the flag or the once-a-day limit says not to. Returns what was
/// found and, for a newer version, the update to install later. A manual check ignores the daily
/// limit, but not the flag. The time is recorded only when the server answered.
async fn check_update(
    updater: &Updater,
    settings: &SettingsStore,
    manual: bool,
    now: u64,
) -> CmdResult<(UpdateOutcome, Option<Update>)> {
    let current = settings.get();
    if !current.flags.update_check {
        return Ok((UpdateOutcome::Disabled, None));
    }
    if !manual && !current.update_check_due(now) {
        return Ok((UpdateOutcome::NotDue, None));
    }
    let found = updater.check().await.map_err(update_error)?;
    settings.set_last_update_check(now)?;
    Ok(match found {
        None => (UpdateOutcome::UpToDate, None),
        Some(update) => (
            UpdateOutcome::Available {
                version: update.version.clone(),
                current_version: update.current_version.clone(),
                notes: update.body.clone(),
            },
            Some(update),
        ),
    })
}

/// Looks for a newer version. The UI calls it with `manual` false at launch and true from the
/// "Check for updates" button.
#[tauri::command]
#[specta::specta]
pub async fn update_check(
    updater: State<'_, UpdateState>,
    ctx: State<'_, AppContext>,
    state: State<'_, AppState>,
    manual: bool,
) -> CmdResult<UpdateOutcome> {
    let (outcome, update) =
        check_update(updater_of(&updater)?, ctx.settings(), manual, unix_now()).await?;
    if let Some(update) = update {
        *state
            .pending_update
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(update);
    }
    Ok(outcome)
}

/// Downloads the update `update_check` found, checks its signature and installs it. On Windows the
/// installer takes over and the app closes; elsewhere `update_restart` starts the new version.
#[tauri::command]
#[specta::specta]
pub async fn update_install(state: State<'_, AppState>) -> CmdResult<()> {
    let update = state
        .pending_update
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "There is no update to install."))?;
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(update_error)?;
    update.install(bytes).map_err(update_error)
}

/// Starts the app again, after an update was installed. Running jobs stop at their next check, as
/// they do when the app is closed.
#[tauri::command]
#[specta::specta]
pub fn update_restart(jobs: State<'_, JobManager>) {
    jobs.cancel_all();
    tauri::process::restart(&tauri::Env::default());
}
#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use gtp_app::{FlagsPatch, SettingsPatch};
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

    use super::*;

    /// A throwaway key pair made only for these tests; the private key was not kept.
    const PUBKEY: &str = include_str!("../../tests/fixtures/updater/test.key.pub");
    const PAYLOAD: &[u8] = include_bytes!("../../tests/fixtures/updater/payload.bin");
    /// Signed for version 9.9.9 (`tauri signer sign --app-version 9.9.9`).
    const SIGNATURE: &str = include_str!("../../tests/fixtures/updater/payload.bin.sig");

    /// A server on a free local port that answers `/latest.json` with `manifest` and `/payload`
    /// with `payload`, and counts the requests it gets.
    struct Server {
        url: String,
        requests: Arc<AtomicUsize>,
    }

    fn serve(version: &str, signature: &str, payload: &'static [u8]) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let manifest = serde_json::json!({
            "version": version,
            "notes": "What is new",
            "pub_date": "2026-11-01T00:00:00Z",
            "platforms": { "test": {
                "signature": signature.trim(),
                "url": format!("http://127.0.0.1:{port}/payload"),
            } },
        })
        .to_string();
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut head = [0u8; 2048];
                let n = stream.read(&mut head).unwrap_or(0);
                counter.fetch_add(1, Ordering::SeqCst);
                let path = String::from_utf8_lossy(&head[..n])
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .to_owned();
                let (kind, body): (&str, &[u8]) = if path == "/latest.json" {
                    ("application/json", manifest.as_bytes())
                } else {
                    ("application/octet-stream", payload)
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(body);
            }
        });
        Server {
            url: format!("http://127.0.0.1:{port}/latest.json"),
            requests,
        }
    }

    /// An app on the mock runtime with the updater plugin pointed at `server`, as `tauri.conf.json`
    /// points it at GitHub.
    fn app_for(server: &Server) -> tauri::App<MockRuntime> {
        let mut context = mock_context(noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({
                "endpoints": [server.url],
                "pubkey": PUBKEY.trim(),
                "requireSignedVersion": true,
                "dangerousInsecureTransportProtocol": true,
            }),
        );
        mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().target("test").build())
            .build(context)
            .unwrap()
    }

    fn updater_of_app(app: &tauri::App<MockRuntime>) -> Updater {
        use tauri_plugin_updater::UpdaterExt;
        app.updater().unwrap()
    }

    fn store(label: &str) -> (SettingsStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "gtp-update-{label}-{}-{}",
            std::process::id(),
            gtp_core::FrameId::new()
        ));
        (SettingsStore::load(&dir.join("settings.json")).0, dir)
    }

    fn check(
        app: &tauri::App<MockRuntime>,
        settings: &SettingsStore,
        manual: bool,
        now: u64,
    ) -> (UpdateOutcome, Option<Update>) {
        tauri::async_runtime::block_on(check_update(&updater_of_app(app), settings, manual, now))
            .unwrap()
    }

    #[test]
    fn a_newer_version_is_offered_and_its_package_verifies() {
        let server = serve("9.9.9", SIGNATURE, PAYLOAD);
        let app = app_for(&server);
        let (settings, dir) = store("newer");
        let (outcome, update) = check(&app, &settings, false, 1_000);
        let UpdateOutcome::Available { version, notes, .. } = outcome else {
            unreachable!("expected an update, got {outcome:?}");
        };
        assert_eq!(version, "9.9.9");
        assert_eq!(notes.as_deref(), Some("What is new"));
        let update = update.unwrap();
        let bytes = tauri::async_runtime::block_on(update.download(|_, _| {}, || {})).unwrap();
        assert_eq!(bytes, PAYLOAD);
        assert_eq!(settings.get().last_update_check, Some(1_000));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_version_that_is_not_newer_is_not_offered() {
        for version in ["0.0.0", "0.0.1"] {
            let server = serve(version, SIGNATURE, PAYLOAD);
            let app = app_for(&server);
            let (settings, dir) = store("older");
            let (outcome, update) = check(&app, &settings, false, 1_000);
            assert_eq!(outcome, UpdateOutcome::UpToDate);
            assert!(update.is_none());
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_tampered_package_is_rejected() {
        static TAMPERED: [u8; 8] = *b"tampered";
        let server = serve("9.9.9", SIGNATURE, &TAMPERED);
        let app = app_for(&server);
        let (settings, dir) = store("tampered");
        let (_, update) = check(&app, &settings, false, 1_000);
        let err =
            tauri::async_runtime::block_on(update.unwrap().download(|_, _| {}, || {})).unwrap_err();
        assert_eq!(update_error(err).code, ErrorCode::InvalidInput);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_manifest_that_announces_another_version_than_the_one_signed_is_rejected() {
        // The package is genuine but was signed for 9.9.9: a manifest that pairs it with a
        // different (say, newer) version number is a downgrade attempt.
        let server = serve("9.9.10", SIGNATURE, PAYLOAD);
        let app = app_for(&server);
        let (settings, dir) = store("version-mismatch");
        let (_, update) = check(&app, &settings, false, 1_000);
        let err =
            tauri::async_runtime::block_on(update.unwrap().download(|_, _| {}, || {})).unwrap_err();
        assert_eq!(update_error(err).code, ErrorCode::InvalidInput);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_manifest_with_a_wrong_signature_is_rejected() {
        // A signature of another file: well formed, but not made for this package.
        let server = serve("9.9.9", "dW50cnVzdGVkIGNvbW1lbnQ6IG5vdA==", PAYLOAD);
        let app = app_for(&server);
        let (settings, dir) = store("wrong-signature");
        let (_, update) = check(&app, &settings, false, 1_000);
        let err =
            tauri::async_runtime::block_on(update.unwrap().download(|_, _| {}, || {})).unwrap_err();
        assert_eq!(update_error(err).code, ErrorCode::InvalidInput);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nothing_is_asked_when_the_flag_is_off_even_for_a_manual_check() {
        let server = serve("9.9.9", SIGNATURE, PAYLOAD);
        let app = app_for(&server);
        let (settings, dir) = store("off");
        settings
            .update(&SettingsPatch {
                flags: Some(FlagsPatch {
                    update_check: Some(false),
                    ..FlagsPatch::default()
                }),
                ..SettingsPatch::default()
            })
            .unwrap();
        for manual in [false, true] {
            let (outcome, update) = check(&app, &settings, manual, 1_000_000);
            assert_eq!(outcome, UpdateOutcome::Disabled);
            assert!(update.is_none());
        }
        assert_eq!(server.requests.load(Ordering::SeqCst), 0);
        assert_eq!(settings.get().last_update_check, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_launch_check_waits_a_day_but_a_manual_check_does_not() {
        let server = serve("9.9.9", SIGNATURE, PAYLOAD);
        let app = app_for(&server);
        let (settings, dir) = store("daily");
        assert!(matches!(
            check(&app, &settings, false, 1_000).0,
            UpdateOutcome::Available { .. }
        ));
        assert_eq!(server.requests.load(Ordering::SeqCst), 1);

        assert_eq!(
            check(&app, &settings, false, 2_000).0,
            UpdateOutcome::NotDue
        );
        assert_eq!(server.requests.load(Ordering::SeqCst), 1);

        assert!(matches!(
            check(&app, &settings, true, 2_000).0,
            UpdateOutcome::Available { .. }
        ));
        assert_eq!(server.requests.load(Ordering::SeqCst), 2);

        // A day after the last answer, which the manual check at 2_000 renewed.
        let next_day = 2_000 + gtp_app::UPDATE_CHECK_INTERVAL_SECS;
        assert!(matches!(
            check(&app, &settings, false, next_day).0,
            UpdateOutcome::Available { .. }
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unreachable_server_is_an_io_error_and_does_not_count_as_a_check() {
        let server = serve("9.9.9", SIGNATURE, PAYLOAD);
        let app = app_for(&Server {
            url: "http://127.0.0.1:9/latest.json".into(),
            requests: server.requests.clone(),
        });
        let (settings, dir) = store("offline");
        let err = tauri::async_runtime::block_on(check_update(
            &updater_of_app(&app),
            &settings,
            true,
            1_000,
        ))
        .err()
        .unwrap();
        assert_eq!(err.code, ErrorCode::Io);
        assert_eq!(settings.get().last_update_check, None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
