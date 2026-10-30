//! Negative-to-positive maths: film-base measurement and normalization today; inversion and
//! frame detection follow. Pure functions over `ImageF32` (docs/architecture.md section 3).

mod film_base;
mod normalize;

pub use film_base::{auto_film_base, roll_base, sample_film_base, BaseEstimate, Rect};
pub use normalize::{normalize, EPSILON};
