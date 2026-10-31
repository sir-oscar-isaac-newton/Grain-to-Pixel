//! Library entities (docs/specification.md section 4). Plain data: no I/O, no database types.

mod frame;
mod roll;
mod source;
mod tag;

pub use frame::{Frame, FrameKind};
pub use roll::{check_iso, check_shot_range, FilmFormat, Roll, FILM_BASE_MIN_CONFIDENCE, MAX_ISO};
pub use source::{SourceFile, SourceRole};
pub use tag::Tag;
