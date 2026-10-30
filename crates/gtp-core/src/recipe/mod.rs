//! The Recipe: every edit of a frame as plain, versioned data (docs/specification.md section 4.1).
//! "Auto" functions fill a Recipe; there is no hidden state.

mod clamp;
mod migrate;
mod v1;

pub use clamp::{
    ANGLE_DEG_MAX, BASE_MAX, BASE_MIN, CURVE_POINTS_MAX, DUST_GROW_PX_MAX, EXPOSURE_EV_MAX,
    INVERT_GAP, INVERT_MAX, MIN_CROP_SIZE, SATURATION_MAX, SLIDER_MAX,
};
pub use migrate::{migrate, CURRENT_VERSION};
pub use v1::{
    Base, BaseSource, Color, Crop, Curves, Dust, Geometry, Invert, Output, OutputSpace, Recipe,
    Tone,
};
