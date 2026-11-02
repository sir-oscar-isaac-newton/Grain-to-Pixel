//! Ingest adapters: file import, camera tethering (RGB triplets), Nikon Coolscan LS-50 (RGB + IR) via `nkscan`.

pub mod capture;
pub mod channel;
pub mod files;
pub mod fixture;
pub mod scanner;
pub mod triplet;

#[cfg(any(test, feature = "testing"))]
pub use capture::testing;
pub use capture::{read_camera_file, read_capture_time, read_exif_file};
pub use channel::{
    dominant_channel, guess_from_means, ChannelGuess, TripletChannel, MIN_LEVEL, MIN_RATIO,
};
pub use files::{scan_tiff_candidates, Grouped, Rejected, TiffCandidate};
pub use fixture::{FixtureSource, FIXTURE_DEVICE_ID};
pub use scanner::{
    CancelToken, DeviceInfo, RawScan, ScanMeta, ScanParams, ScannerSource, DEFAULT_DPI, MAX_FRAMES,
    SAMPLE_COUNTS, SUPPORTED_DPI,
};
pub use triplet::{
    group_triplets, CaptureTime, ExifFile, TripletCandidate, TripletFile, TripletProblem,
    MAX_SHOT_GAP_MS,
};
