//! Application layer (ADR 0005): the only crate that combines the feature crates. Today it holds
//! the render pipeline (`pipeline::render`, `analyze`, the proxy builder and display conversion)
//! and the job manager (`jobs`); use-case services follow. It does not depend on Tauri.

pub mod jobs;
pub mod pipeline;

pub use jobs::{JobCtx, JobKind, JobManager, JobSink, PROGRESS_STEP};
pub use pipeline::{
    analyze, build_proxy, histogram, render, to_rgba8_srgb, Analysis, Histogram, PipelineInput,
    RenderTarget, Stage, PREVIEW_EDGE, PROXY_EDGE, STAGE_ORDER,
};
