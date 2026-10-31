//! Backend-to-UI events, declared with `tauri_specta` so their payloads are typed in the bindings
//! and registered in `ipc.rs`. `library://changed` and the capture events follow with their issues.

use gtp_core::{AppError, JobId, JobResult, RollId};
use serde::Serialize;
use specta::Type;

/// A job moved forward. `fraction` runs from 0 to 1 within `stage`.
#[derive(Debug, Clone, PartialEq, Serialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "job://progress")]
pub struct JobProgress {
    pub id: JobId,
    pub stage: String,
    pub fraction: f32,
}

/// A job finished. Sent once per job, after its last progress event.
#[derive(Debug, Clone, PartialEq, Serialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "job://done")]
pub struct JobDone {
    pub id: JobId,
    pub result: JobResult,
}

/// Frames were added, changed or removed (by a job such as an import): refresh the roll, or
/// everything when `roll_id` is `null`.
#[derive(Debug, Clone, PartialEq, Serialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "library://changed")]
pub struct LibraryChanged {
    pub roll_id: Option<RollId>,
}

/// A job failed or was cancelled (`error.code` is `Cancelled`). Sent once per job, instead of
/// `job://done`.
#[derive(Debug, Clone, PartialEq, Serialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "job://error")]
pub struct JobError {
    pub id: JobId,
    pub error: AppError,
}
