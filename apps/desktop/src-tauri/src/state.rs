//! Shared application state managed by Tauri. The job manager is managed on its own (it needs the
//! app handle, so it is created at setup); the open library follows with L-03.

use std::sync::Mutex;

use crate::crash::CrashNote;

#[derive(Default)]
pub struct AppState {
    /// Crash notes found at startup, handed to the UI once by `crash_notes_take`.
    pub crash_notes: Mutex<Vec<CrashNote>>,
    /// The update the last check found, installed when the user asks for it.
    pub pending_update: Mutex<Option<tauri_plugin_updater::Update>>,
}
