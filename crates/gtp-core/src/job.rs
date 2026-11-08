//! What a finished background job hands back to the UI (the `job://done` event). Each job kind adds
//! its own variant when it arrives (import, export, scan, ...).

use serde::Serialize;

use crate::ids::{FrameId, RollId};
use crate::model::{ExposureInfo, SourceRole, TripletChannel};

/// The result of a job that finished without error.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind")]
pub enum JobResult {
    /// The job did its work and has nothing to report.
    Finished,
    /// A file import (L-05).
    Imported(ImportResult),
    /// A library verify (L-07).
    Verified(VerifyResult),
    /// An export (P-12).
    Exported(ExportResult),
    /// An automatic Scanlight level calibration (H-17).
    Calibrated(CalibrationResult),
}

/// What an automatic calibration found. Nothing is saved: the user reviews it and saves it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CalibrationResult {
    /// The level to use for red, green and blue (1 to 255).
    pub levels: [u8; 3],
    /// What the brightest pixels (99.9th percentile) of each colour read at its level, 0 to 1.
    pub reached: [f32; 3],
    /// The shots each colour took.
    pub shots: [u8; 3],
    /// Things worth knowing about a calibration that still succeeded.
    pub warnings: Vec<CalibrationWarning>,
    /// The shutter time, ISO and aperture of the last shot, as its EXIF says (all shots share
    /// them); saved with the levels like a test shot's, so a later capture with other settings
    /// can warn.
    pub exposure: ExposureInfo,
}

/// Something to know about a successful calibration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CalibrationWarning {
    /// The level is so low that one step of it changes the brightness by a large share, so it
    /// cannot be set finely.
    CoarseLevel { channel: TripletChannel, level: u8 },
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
    /// Things worth knowing about imported frames that still were imported (RGB triplets).
    pub warnings: Vec<ImportWarning>,
    /// The roll's film base is a low-confidence guess after this import: the UI points to the
    /// picker in Develop.
    pub base_is_guess: bool,
    /// This import replaced the roll's guessed film base with a better measured one, so the roll's
    /// other frames look different now.
    pub base_replaced: bool,
    /// Frames of the roll whose automatic recipe got the roll's colour balance when the job ended
    /// (P-22), so their thumbnails are made again. Empty when every frame had it from the start.
    pub rebalanced: Vec<FrameId>,
}

/// Something to know about an imported frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ImportWarning {
    pub frame_id: FrameId,
    pub kind: ImportWarningKind,
}

/// What an import warns about. Not errors: the frame was imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportWarningKind {
    /// The three exposures of a triplet used different shutter times.
    ExposureTimeMismatch,
    /// ... different ISO values.
    IsoMismatch,
    /// ... different apertures.
    ApertureMismatch,
    /// More of the lit channel than expected is at full scale: `per_mille` of its samples (parts
    /// per thousand).
    Clipped {
        channel: TripletChannel,
        per_mille: u32,
    },
    /// The roll's flat field is for frames of another size, so this frame was imported without it.
    FlatFieldSkipped,
    /// None of the job's frames had a usable neutral, so the roll has no colour balance (the frame
    /// named is the first of the job).
    BalanceSkipped,
    /// The light was warm while this frame was captured: the highest LED temperature seen, in whole
    /// degrees Celsius, above the warn limit (H-08).
    HighTemperature { temp_c: u32 },
    /// The light's supply voltage sagged while this frame was captured: the lowest reading, in
    /// millivolts (H-08).
    LowSupply { vbus_mv: u32 },
    /// The camera settings (shutter, ISO or aperture) of this capture differ from those the levels
    /// in use were saved with (H-15).
    ExposureDiffersFromLevels,
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

/// What an export did.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ExportResult {
    /// The files written, in export order.
    pub files: Vec<ExportedFile>,
    /// Frames that were not exported, with the reason. The others were.
    pub failed: Vec<FailedExport>,
    /// Milliseconds each exported frame took (budget: 10,000 per 21 MP frame, spec section 11).
    pub frame_ms: Vec<u32>,
}

/// A file an export wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ExportedFile {
    pub frame_id: FrameId,
    /// The file, as an absolute path.
    pub path: String,
}

/// A frame an export could not write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FailedExport {
    pub frame_id: FrameId,
    /// What went wrong, in words for the person.
    pub message: String,
    /// Technical detail for the log or a tooltip.
    pub details: Option<String>,
}
