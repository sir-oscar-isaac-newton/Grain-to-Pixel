//! Application layer (ADR 0005): the only crate that combines the feature crates. Today it holds
//! the render pipeline (`pipeline::render`, `analyze`, the proxy builder and display conversion);
//! use-case services and the job manager follow. It does not depend on Tauri.

pub mod pipeline;

pub use pipeline::{
    analyze, build_proxy, histogram, render, to_rgba8_srgb, Analysis, Histogram, PipelineInput,
    RenderTarget, Stage, PREVIEW_EDGE, PROXY_EDGE, STAGE_ORDER,
};
