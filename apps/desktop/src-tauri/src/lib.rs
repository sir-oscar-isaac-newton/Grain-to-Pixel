//! Tauri adapter: registers commands, events and app state. It validates input, calls
//! application services and maps errors; it holds no business logic (docs/architecture.md section 3).

mod commands;
mod crash;
mod error;
mod events;
mod ipc;
mod logging;
mod sink;
mod state;
#[cfg(test)]
mod testkit;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gtp_app::{AppContext, Hardware, JobManager, SharedLight};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, RunEvent, Runtime, WindowEvent};

pub use ipc::export_bindings;

/// Environment variable naming a file the app writes once its page has loaded. Used only by the
/// smoke test (tools/scripts/smoke) as a "started and rendered" marker; absent in normal use.
const READY_FILE_ENV: &str = "GTP_READY_FILE";

/// How often the temperature watchdog reads the light: the light itself reports every 200 ms.
const LIGHT_POLL: Duration = Duration::from_millis(200);

fn ready_file_path() -> Option<PathBuf> {
    std::env::var_os(READY_FILE_ENV).map(PathBuf::from)
}

fn signal_ready(path: &Path) -> std::io::Result<()> {
    std::fs::write(path, env!("CARGO_PKG_VERSION"))
}

/// Builds and runs the desktop application.
pub fn run() -> tauri::Result<()> {
    // Before anything starts a thread: libgphoto2 reads its driver paths from the environment
    // (macOS builds with the `tether` feature; a no-op everywhere else).
    // Logging has not started yet: what it did is logged in `setup`.
    let camera_libraries_from_bundle = gtp_app::prepare_camera_libraries();
    let builder = ipc::builder::<tauri::Wry>();
    let handler = ipc::invoke_handler(&builder);
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Only the typed `update_*` commands reach it: no `updater:` permission is granted, so the
        // page cannot call the plugin directly.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state::AppState::default())
        .on_page_load(|_webview, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                if let Some(path) = ready_file_path() {
                    // Best effort: a missing marker makes the smoke test fail, which is the signal.
                    let _ = signal_ready(&path);
                }
            }
        })
        .invoke_handler(handler)
        .on_window_event(|window, event| on_window_event(window.app_handle(), event))
        .setup(move |app| {
            builder.mount_events(app);
            // The devices come first: the panic hook needs the light.
            let hardware = Hardware::from_env();
            start_observability(app, hardware.light.clone());
            gtp_app::log_camera_libraries(camera_libraries_from_bundle);
            log_hardware(&hardware);
            let sink = Arc::new(sink::TauriJobSink::new(app.handle().clone()));
            app.manage(JobManager::new(sink.clone()));
            app.manage(sink::CaptureEventsState(sink.clone()));
            app.manage(sink::EventsState(sink));
            app.manage(app_context(app));
            app.manage(update_state(app));
            app.manage(hardware);
            app.manage(gtp_app::services::scan_preview::ScanPreviewService::default());
            // Without the watchdog the light's temperature limit would not be kept: no start.
            start_light_watch(app.handle().clone())?;
            Ok(())
        })
        .build(tauri::generate_context!())?
        .run(|app, event| on_run_event(app, &event));
    Ok(())
}

/// Switches the Scanlight off when the app quits (and stops running jobs on
/// exit). Closing the window only switches it off, so a window opened again on macOS can use it;
/// quitting also refuses every colour from then on, so a capture that is still running cannot
/// light the next one while the app goes away.
fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    match event {
        RunEvent::ExitRequested { .. } => switch_light_off(app, false),
        RunEvent::Exit => {
            // Running jobs stop at their next check instead of being cut off mid-write.
            if let Some(jobs) = app.try_state::<JobManager>() {
                jobs.cancel_all();
            }
            switch_light_off(app, true);
        }
        _ => {}
    }
}

/// The window part of [`on_run_event`]: the window is closing or gone.
fn on_window_event<R: Runtime>(app: &AppHandle<R>, event: &WindowEvent) {
    if matches!(
        event,
        WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
    ) {
        switch_light_off(app, false);
    }
}

fn switch_light_off<R: Runtime>(app: &AppHandle<R>, quitting: bool) {
    let Some(hardware) = app.try_state::<Hardware>() else {
        return;
    };
    if quitting {
        hardware.light.shut_down();
    } else if let Err(err) = hardware.light.off() {
        tracing::error!(%err, "switching the light off failed");
    }
}

/// Starts the thread that reads the light every [`LIGHT_POLL`], emits `light://status` and
/// switches the light off above the profile's abort temperature (the watchdog, spec section 6.1).
fn start_light_watch<R: Runtime>(app: AppHandle<R>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("light-watch".into())
        .spawn(move || loop {
            std::thread::sleep(LIGHT_POLL);
            // A failed tick (nothing should panic here) must not end the watchdog.
            let tick = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                watch_light_once(&app);
            }));
            if tick.is_err() {
                tracing::error!("a light watchdog tick panicked");
            }
        })
        .map(|_| ())
}

/// One watchdog tick with the app's own state; nothing happens before the state is managed.
fn watch_light_once<R: Runtime>(app: &AppHandle<R>) {
    let (Some(hardware), Some(ctx), Some(events)) = (
        app.try_state::<Hardware>(),
        app.try_state::<AppContext>(),
        app.try_state::<sink::CaptureEventsState>(),
    ) else {
        return;
    };
    let abort_c = ctx.settings().capture_profile().temp_abort_c;
    hardware.watch_light(abort_c, events.0.as_ref());
}

/// The shared context with `settings.json` from the OS app-config folder. Without one (very
/// unusual) settings live in the temp folder for this run, so the app still starts.
fn app_context(app: &tauri::App) -> AppContext {
    let dir = app.path().app_config_dir().unwrap_or_else(|err| {
        tracing::warn!(%err, "no app config folder; settings are kept in the temp folder");
        std::env::temp_dir().join("grain-to-pixel")
    });
    let (ctx, note) = AppContext::new(&dir.join("settings.json"));
    if let Some(note) = note {
        tracing::warn!(%note, "settings reset to defaults");
    }
    ctx
}

/// Logs that the devices are fixtures (`GTP_FAKE_HW=1`).
fn log_hardware(hardware: &Hardware) {
    if let Some(dir) = hardware.fixture_dir() {
        tracing::info!(folder = %dir.display(), "fake hardware is on: the fixture scanner replays this folder, and the light and camera are fakes");
    }
}

/// The updater, built from `tauri.conf.json`; a failure is logged and leaves the update commands
/// reporting an error, so a bad updater setting never stops the app.
fn update_state(app: &tauri::App) -> commands::update::UpdateState {
    use tauri_plugin_updater::UpdaterExt;
    commands::update::UpdateState(match app.updater() {
        Ok(updater) => Some(updater),
        Err(err) => {
            tracing::warn!(%err, "the updater could not be set up");
            None
        }
    })
}

/// Starts file logging and the panic hook (which also switches `light` off), then collects crash
/// notes from earlier runs. Failures never stop the app: without a log directory it runs with
/// stderr logging only.
fn start_observability(app: &tauri::App, light: SharedLight) {
    let Ok(dir) = app.path().app_log_dir() else {
        // No log folder, but the panic hook still has to switch the light off.
        crash::install_hook(None, Some(light));
        return;
    };
    if let Ok(mut slot) = app.state::<state::AppState>().log_dir.lock() {
        *slot = Some(dir.clone());
    }
    match logging::init(&dir) {
        Ok(guard) => {
            // Dropping the guard would stop the log writer, so it lives as long as the app.
            app.manage(LogGuard(guard));
        }
        Err(err) => {
            // Another subscriber (tests, dev tooling) or an unwritable directory: keep going.
            let _ = tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .try_init();
            tracing::warn!(%err, "file logging is off");
        }
    }
    crash::install_hook(Some(dir.clone()), Some(light));
    match crash::take_unreported(&dir) {
        Ok(notes) => {
            if let Ok(mut slot) = app.state::<state::AppState>().crash_notes.lock() {
                *slot = notes;
            }
        }
        Err(err) => tracing::warn!(%err, "could not read crash notes"),
    }
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "grain to pixel started"
    );
}

/// Keeps the non-blocking log writer alive (see [`start_observability`]).
struct LogGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

#[cfg(test)]
mod tests {
    use gtp_app::fake_capture::{FakeLight, FakeLightHandle, LightSource, LightStatus};
    use gtp_app::shared_light::fake_ports;

    use super::*;

    /// A mock app that manages the app's devices with a scripted light, as `setup` does.
    fn app_with_light() -> (tauri::App<tauri::test::MockRuntime>, FakeLightHandle) {
        let app = tauri::test::mock_app();
        let fake = FakeLight::new();
        let handle = fake.handle();
        let hardware = Hardware::from_vars(Some("1"), Some("none".into())).with_capture(
            SharedLight::new(Box::new(fake), fake_ports),
            None,
            Arc::new(gtp_app::fake_capture::FakeRawDecoder),
        );
        app.manage(hardware);
        (app, handle)
    }

    fn light_on(app: &tauri::App<tauri::test::MockRuntime>, handle: &FakeLightHandle) {
        let mut light = app.state::<Hardware>().light.clone();
        light.set_levels([44, 22, 2]).unwrap();
        assert_eq!(handle.levels(), [44, 22, 2]);
    }

    #[test]
    fn the_light_is_switched_off_when_a_window_closes() {
        let (app, handle) = app_with_light();
        light_on(&app, &handle);
        on_window_event(app.handle(), &WindowEvent::Destroyed);
        assert_eq!(handle.levels(), [0; 3]);
        // Only switched off: it can be used again (a window opened again on macOS).
        let mut light = app.state::<Hardware>().light.clone();
        assert!(light.set_levels([1, 0, 0]).is_ok());
    }

    #[test]
    fn the_light_is_switched_off_on_exit_and_cannot_be_lit_again() {
        let (app, handle) = app_with_light();
        light_on(&app, &handle);
        on_run_event(app.handle(), &RunEvent::Exit);
        assert_eq!(handle.levels(), [0; 3]);
        let mut light = app.state::<Hardware>().light.clone();
        assert!(light.set_levels([1, 0, 0]).is_err());
        // Other events change nothing.
        let (app, handle) = app_with_light();
        light_on(&app, &handle);
        on_run_event(app.handle(), &RunEvent::Ready);
        assert_eq!(handle.levels(), [44, 22, 2]);
    }

    #[test]
    fn a_missing_hardware_state_does_not_stop_the_exit_hook() {
        let app = tauri::test::mock_app();
        on_run_event(app.handle(), &RunEvent::Exit);
        watch_light_once(app.handle());
    }

    #[test]
    fn a_watchdog_tick_with_the_apps_own_state_trips_a_hot_light() {
        let dir = gtp_core::testing::TempDir::new("watch");
        let (app, handle) = app_with_light();
        ipc::builder::<tauri::test::MockRuntime>().mount_events(&app);
        let (ctx, _) = AppContext::new(&dir.join("settings.json"));
        app.manage(ctx);
        let sink = Arc::new(sink::TauriJobSink::new(app.handle().clone()));
        app.manage(sink::CaptureEventsState(sink));
        light_on(&app, &handle);
        handle.set_status(LightStatus {
            temp_c: Some(80.0),
            vbus_mv: Some(5150),
            connected: true,
        });
        watch_light_once(app.handle());
        assert_eq!(handle.levels(), [0; 3]);
        assert!(app.state::<Hardware>().light.is_tripped());
    }

    #[test]
    fn signal_ready_writes_the_app_version() {
        let dir = gtp_core::testing::TempDir::new("ready");
        let path = dir.join("ready.txt");
        signal_ready(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn signal_ready_reports_an_unwritable_path() {
        let path = std::env::temp_dir()
            .join("gtp-no-such-dir")
            .join("ready.txt");
        assert!(signal_ready(&path).is_err());
    }
}
