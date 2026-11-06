//! Edit stages over linear `ImageF32` buffers: colour balance and tone (in place) and geometry
//! (a new buffer), IR dust removal and output sharpening. Per-pixel loops run in parallel with `rayon`; each
//! pixel or output row is independent, so the result is the same as on one thread. Every tuning constant is a named, documented `pub const`
//! so it can be calibrated against reference renders (issue Q-03).

mod color;
mod curve;
mod dust;
mod frontier;
mod geometry;
mod look;
mod sharpen;
mod tone;

pub use color::{
    apply_color, channel_gains, neutral_balance, Area, AXIS_STOPS, TEMPERATURE_STOPS, TINT_STOPS,
};
pub use curve::{Curve, LUT_SIZE};
pub use dust::{
    remove_dust, DustReport, DIAGONAL_WEIGHT, DUST_CLEAR_PERCENTILE, MAX_MASKED_FRACTION,
    MAX_SPOT_FRACTION,
};
pub use frontier::{
    frontier_code, frontier_curve, FrontierParams, FRONTIER_GAMMA, FRONTIER_PIVOT,
    FRONTIER_SATURATION, FRONTIER_SLOPE, GAMMA_RANGE, PIVOT_RANGE, SATURATION_RANGE, SLOPE_RANGE,
};
pub use geometry::{apply_geometry, output_size};
pub use look::{
    film_log_code, film_log_curve, gen5_decode, gen5_encode, look_values, range_stops, srgb_decode,
    LookValues, CURVE_TOLERANCE, FILM_GAMMA, GEN5_A, GEN5_B, GEN5_C, GEN5_D, GEN5_E, GEN5_LIN_CUT,
    GEN5_LOG_CUT, HEADROOM_TOLERANCE, MAX_RANGE_STOPS, MID_GREY_SCENE, MIN_RANGE_STOPS,
};
pub use sharpen::{apply_sharpen, KERNEL_SIGMAS, SHARPEN_GAMMA};
pub use tone::{
    apply_tone, BLACKS_RANGE, DEHAZE_ADD_VEIL, DEHAZE_SATURATION, DEHAZE_VEIL_PERCENTILE,
    HIGHLIGHT_RANGE_STOPS, MID_GREY, REGION_STOPS, SHADOW_RANGE_STOPS, WHITES_RANGE,
};

use gtp_core::{AppError, ErrorCode, ImageF32};

/// Largest linear sample the stages accept; larger values are limited to it. Real images stay far
/// below it, and the limit keeps every stage free of infinity.
pub const MAX_SAMPLE: f32 = 1.0e6;

/// Rec.2020 luma of a linear RGB pixel.
pub(crate) fn luma(rgb: [f32; 3]) -> f32 {
    let [wr, wg, wb] = gtp_core::recipe::REC2020_LUMA;
    wr * rgb[0] + wg * rgb[1] + wb * rgb[2]
}

/// Scales the pixel's distance from its luma by `factor` (0 is grey, 1 unchanged); never below 0.
pub(crate) fn saturate(pixel: &mut [f32; 3], factor: f32) {
    let y = luma(*pixel);
    for c in pixel.iter_mut() {
        *c = (y + (*c - y) * factor).max(0.0);
    }
}

pub(crate) fn require_rgb(image: &ImageF32, stage: &str) -> Result<(), AppError> {
    if image.channels == 3 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::InvalidInput,
            format!("The {stage} stage needs a 3-channel image."),
        )
        .with_details(format!("{} channels", image.channels)))
    }
}
