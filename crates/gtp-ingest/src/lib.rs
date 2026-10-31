//! Ingest adapters: file import, camera tethering (RGB triplets), Nikon Coolscan LS-50 (RGB + IR) via `nkscan`.

pub mod files;

pub use files::{scan_tiff_candidates, Grouped, Rejected, TiffCandidate};
