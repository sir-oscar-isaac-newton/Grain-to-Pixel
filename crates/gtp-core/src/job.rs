//! What a finished background job hands back to the UI (the `job://done` event). Each job kind adds
//! its own variant when it arrives (import, export, scan, ...).

use serde::Serialize;

/// The result of a job that finished without error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind")]
pub enum JobResult {
    /// The job did its work and has nothing to report.
    Finished,
}
