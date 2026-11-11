//! The Recipe: every edit of a frame as plain, versioned data (docs/specification.md section 4.1).
//! "Auto" functions fill a Recipe; there is no hidden state.

mod clamp;
mod fields;
mod migrate;
mod v1;

pub use clamp::{
    ANGLE_DEG_MAX, BASE_MAX, BASE_MIN, CURVE_POINTS_MAX, DUST_GROW_PX_MAX, EXPOSURE_EV_MAX,
    INVERT_GAP, INVERT_MAX, MIN_CROP_SIZE, MONO_WEIGHT_MAX, NOISE_AMOUNT_MAX, SATURATION_MAX,
    SHARPEN_AMOUNT_MAX, SHARPEN_RADIUS_MAX_PX, SHARPEN_RADIUS_MIN_PX, SLIDER_MAX,
    SPLIT_TONE_HUE_MAX, SPLIT_TONE_STRENGTH_MAX, VIGNETTE_MIDPOINT_MAX,
};
pub use fields::RecipeField;
pub use migrate::{migrate, CURRENT_VERSION};
pub use v1::{
    Base, BaseSource, Color, Crop, CurvePoint, Curves, Dust, Geometry, Invert, Mixer, Mono, Noise,
    Output, OutputSpace, Recipe, Sharpen, SplitTone, Tone, Vignette, DEFAULT_SHARPEN_RADIUS_PX,
    MIXER_BANDS, REC2020_LUMA,
};
