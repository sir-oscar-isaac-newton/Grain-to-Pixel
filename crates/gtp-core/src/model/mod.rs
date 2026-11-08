//! Library entities (docs/specification.md section 4). Plain data: no I/O, no database types.

mod channel;
mod exposure;
mod flat_field;
mod frame;
mod roll;
mod search;
mod source;
mod tag;

pub use channel::TripletChannel;
pub use exposure::{ExposureInfo, ExposureRatio};
pub use flat_field::FlatField;
pub use frame::{Frame, FrameKind};
pub use roll::{
    check_iso, check_shot_range, partial_date_span, BaseChange, FilmFormat, FrameSize, Roll,
    FILM_BASE_MIN_CONFIDENCE, MAX_ISO,
};
pub use search::SearchQuery;
pub use source::{SourceFile, SourceRole};
pub use tag::Tag;
