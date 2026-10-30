//! Shared application state managed by Tauri. Filled in by later issues (open library, job manager).

use std::sync::Mutex;

use crate::crash::CrashNote;

#[derive(Debug, Default)]
pub struct AppState {
    /// Crash notes found at startup, handed to the UI once by `crash_notes_take`.
    pub crash_notes: Mutex<Vec<CrashNote>>,
}
