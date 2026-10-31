//! What a finished background job hands back to the UI (the `job://done` event). Each job kind adds
//! its own variant when it arrives (import, export, scan, ...).

use serde::Serialize;

use crate::ids::{FrameId, RollId};
use crate::model::SourceRole;

/// The result of a job that finished without error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind")]
pub enum JobResult {
    /// The job did its work and has nothing to report.
    Finished,
    /// A file import (L-05).
    Imported(ImportResult),
    /// A library verify (L-07).
    Verified(VerifyResult),
}

/// What a file import did.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ImportResult {
    /// The new frames, in import order.
    pub frames: Vec<FrameId>,
    /// Files that were not imported, with the reason.
    pub skipped: Vec<SkippedFile>,
    /// Milliseconds each imported frame took (budget: 8,000 per frame, spec section 11).
    pub frame_ms: Vec<u32>,
}

/// A picked file that was not imported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SkippedFile {
    /// The file as picked, for display.
    pub path: String,
    pub reason: SkipReason,
    /// Technical detail for the log or a tooltip.
    pub details: Option<String>,
}

/// Why a file was not imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SkipReason {
    /// Not a `.tif` or `.tiff` file.
    NotTiff,
    /// An infrared file (`*_ir.tif`) whose scan was not picked, or one the scan does not need.
    OrphanIr,
    /// The file could not be opened or read: missing, no permission, a disk error.
    Unreadable,
    /// Larger than the app accepts.
    TooLarge,
    /// Not a TIFF the app imports: damaged contents, 8-bit, gray, an unspecified extra channel,
    /// an infrared plane of another size.
    Unsupported,
    /// The original written to the library did not read back identical.
    VerifyFailed,
}

/// What a library verify found. It only reports; nothing is changed or deleted.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct VerifyResult {
    /// Original files examined.
    pub files_checked: u32,
    /// Bytes of the originals as recorded at import.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub bytes_checked: u64,
    /// Originals that are missing, changed or unreadable, in library order.
    pub problems: Vec<VerifyProblem>,
    /// Frames whose thumbnail file is missing (a rebuildable cache, not an error).
    pub thumbnails_missing: u32,
    /// Folders under `originals/` that belong to no roll or frame of the catalog and are not
    /// empty, relative to the library (for example left by a crash). Never deleted.
    pub stray_folders: Vec<String>,
}

/// One original that did not check out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct VerifyProblem {
    pub frame_id: FrameId,
    pub roll_id: RollId,
    pub role: SourceRole,
    /// The file, relative to the library.
    pub rel_path: String,
    pub reason: VerifyReason,
    /// Technical detail for the log or a tooltip.
    pub details: Option<String>,
}

/// What is wrong with an original.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum VerifyReason {
    /// The file is gone.
    Missing,
    /// Its contents differ from what was recorded at import (damaged or replaced).
    Changed,
    /// Its size differs from what was recorded, so it was not even hashed.
    WrongSize,
    /// It exists but cannot be read (permissions, a disk error, in use), or its recorded path
    /// points outside the library.
    Unreadable,
}
