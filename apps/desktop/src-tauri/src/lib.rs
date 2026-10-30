//! Tauri adapter: registers commands, events and app state. It validates input, calls
//! application services and maps errors; it holds no business logic (docs/architecture.md section 3).

mod commands;
mod error;
mod events;
mod ipc;
mod state;

pub use ipc::export_bindings;

/// Builds and runs the desktop application.
pub fn run() -> tauri::Result<()> {
    let builder = ipc::builder();
    tauri::Builder::default()
        .manage(state::AppState::default())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())
}
