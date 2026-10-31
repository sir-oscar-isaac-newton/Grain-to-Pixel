//! Shared domain types: ids, images, Frame / Roll / Tag, the versioned Recipe (edit parameters) and
//! errors. No I/O (docs/architecture.md section 3.4, docs/specification.md section 4).

pub mod error;
pub mod ids;
pub mod image;
pub mod job;
pub mod model;
pub mod recipe;
#[cfg(feature = "testing")]
pub mod testing;

pub use error::{AppError, ErrorCode};
pub use ids::{FrameId, JobId, RollId, TagId};
pub use image::ImageF32;
pub use job::{
    ImportResult, JobResult, SkipReason, SkippedFile, VerifyProblem, VerifyReason, VerifyResult,
};
pub use model::{FilmFormat, Frame, FrameKind, Roll, SearchQuery, SourceFile, SourceRole, Tag};
pub use recipe::{migrate, Recipe, RecipeField};
