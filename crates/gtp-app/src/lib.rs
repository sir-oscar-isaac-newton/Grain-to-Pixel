//! Application layer (ADR 0005): the only crate that combines the feature crates. It holds the
//! render pipeline (`pipeline::render`, `analyze`, the proxy builder and display conversion), the
//! job manager (`jobs`), the library folder and settings (`context`, `services`). It does not
//! depend on Tauri.

pub mod context;
pub mod fake_capture;
pub mod hardware;
pub mod jobs;
pub mod library_fs;
pub mod paths;
pub mod pipeline;
pub mod services;
pub mod shared_light;

pub use context::{AppContext, ThumbnailRequester};
pub use gtp_capture::{
    CameraInfo, CameraSetting, CameraSettingKind, CaptureProfile, CaptureProfilePatch, LightInfo,
    PortInfo, Region,
};
pub use gtp_edit::{frontier_curve, roll_balance, BalanceMethod, FrontierParams};
pub use gtp_ingest::{
    merge_triplet, DeviceInfo, ScannerSource, TripletCandidate, TripletFile, TripletProblem,
};
pub use gtp_io::RawlerDecoder;
pub use gtp_negative::{BaseEstimate, FrameHint};
pub use hardware::{log_camera_libraries, prepare_camera_libraries, Hardware};
pub use jobs::{JobCtx, JobErrorEntry, JobKind, JobManager, JobSink, LibraryEvents, PROGRESS_STEP};
pub use pipeline::{
    analyze, analyze_with, analyze_with_axes, analyze_with_balance, analyze_with_levels,
    analyze_with_look, apply_balance_axes, build_proxy, histogram, render, to_rgba8_srgb, Analysis,
    Histogram, LevelsPercentiles, PipelineInput, RenderTarget, Stage, PREVIEW_EDGE, PROXY_EDGE,
    STAGE_ORDER,
};
pub use services::capture_control::{CameraSlot, CaptureEvents, StepState, TestShot};
pub use services::capture_levels::{CaptureLevels, LevelsScope, LevelsSource};
pub use services::counters::Counters;
pub use services::flat_field::{apply_flat_field, flat_field_from_exposures};
pub use services::library::{Library, LibraryDetails};
pub use services::settings::{
    Flags, FlagsPatch, Settings, SettingsPatch, SettingsStore, Theme, WindowSize,
    UPDATE_CHECK_INTERVAL_SECS,
};
pub use shared_light::{LightReading, SharedLight};
