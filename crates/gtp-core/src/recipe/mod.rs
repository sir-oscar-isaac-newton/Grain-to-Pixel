//! The Recipe: every edit of a frame as plain, versioned data (docs/specification.md section 4.1).
//! "Auto" functions fill a Recipe; there is no hidden state.

mod clamp;
mod fields;
mod migrate;
mod v1;

pub use clamp::{
    ANGLE_DEG_MAX, BASE_MAX, BASE_MIN, CURVE_POINTS_MAX, DUST_GROW_PX_MAX, EXPOSURE_EV_MAX,
    INVERT_GAP, INVERT_MAX, MIN_CROP_SIZE, MONO_WEIGHT_MAX, SATURATION_MAX, SHARPEN_AMOUNT_MAX,
    SHARPEN_RADIUS_MAX_PX, SHARPEN_RADIUS_MIN_PX, SLIDER_MAX,
};
pub use fields::RecipeField;
pub use migrate::{migrate, CURRENT_VERSION};
pub use v1::{
    Base, BaseSource, Color, Crop, CurvePoint, Curves, Dust, Geometry, Invert, Mono, Output,
    OutputSpace, Recipe, Sharpen, Tone, DEFAULT_SHARPEN_RADIUS_PX, REC2020_LUMA,
};
