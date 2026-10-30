//! Tauri adapter: registers commands, events and app state. It validates input, calls
//! application services and maps errors; it holds no business logic (docs/architecture.md section 3).

mod commands;
mod state;

/// Builds and runs the desktop application.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![commands::system::library_info])
        .run(tauri::generate_context!())
}
