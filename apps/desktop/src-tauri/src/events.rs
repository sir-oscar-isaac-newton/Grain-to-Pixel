//! Backend-to-UI events, declared with `tauri_specta` so their payloads are typed in the bindings
//! and registered in `ipc.rs`.

use gtp_app::StepState;
use gtp_core::{AppError, FrameId, JobId, JobResult, RollId, TripletChannel};
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
    pub frame_ids: Vec<FrameId>,
}

/// A job failed or was cancelled (`error.code` is `Cancelled`). Sent once per job, instead of
/// `job://done`.
#[derive(Debug, Clone, PartialEq, Serialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "job://error")]
pub struct JobError {
    pub id: JobId,
    pub error: AppError,
}

/// A capture moved one channel forward: the colour is on (`lit`), the camera was told to shoot
/// (`fired`) or its picture arrived (`received`). Sent for a test shot and for every frame.
#[derive(Debug, Clone, PartialEq, Serialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "capture://step")]
pub struct CaptureStep {
    pub channel: TripletChannel,
    pub state: StepState,
}

/// What the light reported, every 200 ms while it is connected. `tripped` is true once the
/// temperature watchdog switched the light off for being too hot: it stays refused until the user
/// reconnects it (`light_connect`).
#[derive(Debug, Clone, PartialEq, Serialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "light://status")]
pub struct LightStatus {
    pub temp_c: Option<f32>,
    pub vbus_mv: Option<i32>,
    pub tripped: bool,
}
