//! Application layer (ADR 0005): the only crate that combines the feature crates. It holds the
//! render pipeline (`pipeline::render`, `analyze`, the proxy builder and display conversion), the
//! job manager (`jobs`), the library folder and settings (`context`, `services`). It does not
//! depend on Tauri.

pub mod context;
pub mod jobs;
pub mod library_fs;
pub mod paths;
pub mod pipeline;
pub mod services;

pub use context::AppContext;
pub use jobs::{JobCtx, JobKind, JobManager, JobSink, LibraryEvents, PROGRESS_STEP};
pub use pipeline::{
    analyze, build_proxy, histogram, render, to_rgba8_srgb, Analysis, Histogram, PipelineInput,
    RenderTarget, Stage, PREVIEW_EDGE, PROXY_EDGE, STAGE_ORDER,
};
pub use services::library::{Library, LibraryDetails};
pub use services::settings::{Flags, FlagsPatch, Settings, SettingsPatch, Theme, WindowSize};
