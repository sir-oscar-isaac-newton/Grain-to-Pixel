//! Edit stages over linear `ImageF32` buffers, applied in place: colour balance and tone today;
//! geometry and IR dust removal follow. Every tuning constant is a named, documented `pub const`
//! so it can be calibrated against reference renders (issue Q-03).

mod color;
mod curve;
mod tone;

pub use color::{apply_color, channel_gains, AXIS_STOPS, TEMPERATURE_STOPS, TINT_STOPS};
pub use curve::{Curve, LUT_SIZE};
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
