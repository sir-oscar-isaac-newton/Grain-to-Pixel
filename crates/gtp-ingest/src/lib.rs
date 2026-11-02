//! Ingest adapters: file import, camera tethering (RGB triplets), Nikon Coolscan LS-50 (RGB + IR) via `nkscan`.

pub mod capture;
pub mod files;
pub mod triplet;

#[cfg(any(test, feature = "testing"))]
pub use capture::testing;
pub use capture::{read_capture_time, read_exif_file};
pub use files::{scan_tiff_candidates, Grouped, Rejected, TiffCandidate};
pub use triplet::{
    group_triplets, CaptureTime, ExifFile, TripletCandidate, TripletFile, TripletProblem,
    MAX_SHOT_GAP_MS,
};
