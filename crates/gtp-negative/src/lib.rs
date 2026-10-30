//! Negative-to-positive maths: film-base measurement, normalization and inversion; frame
//! detection follows. Pure functions over `ImageF32` (docs/architecture.md section 3).

mod film_base;
mod invert;
mod normalize;

pub use film_base::{auto_film_base, roll_base, sample_film_base, BaseEstimate, Rect};
pub use invert::{
    auto_levels, invert, levels_or_auto, DEFAULT_HIGH_PERCENTILE, DEFAULT_LOW_PERCENTILE,
};
pub use normalize::{normalize, EPSILON};
