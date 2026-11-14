//! The fixed stage order (docs/specification.md section 4.2, rules/colour-pipeline.md).

use gtp_core::recipe::{BaseSource, Crop, Geometry};
use gtp_core::{AppError, ErrorCode, ImageF32, Recipe};

use super::proxy::resize_plane;
use super::PipelineInput;

/// One step of the render pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// IR dust removal (`dust` in the recipe); skipped without an infrared plane.
    Dust,
    /// Crop to the frame (`geometry.crop`), as an exact pixel copy.
    Crop,
    /// Divide by the film base.
    Normalize,
    /// Density inversion with the recipe's black and white points.
    Invert,
    /// Noise reduction (`noise`): a guided filter in Oklab on the positive, before the colour stage.
    Noise,
    /// Colour axes, temperature, tint, black and white, saturation, vibrance.
    Color,
    /// Per-hue changes of hue, saturation and lightness (`color.mixer`); skipped in black and white.
    Mixer,
    /// Exposure, contrast, highlights, shadows, whites, blacks, dehaze, curves.
    Tone,
    /// Blend the highlights and the shadows towards two tints of the same luma (`color.split_tone`).
    SplitTone,
    /// Straighten, quarter turns and flips (the crop was applied earlier).
    Geometry,
    /// Darken or lighten the corners of the final picture (`tone.vignette`).
    Vignette,
}

/// The order every render runs the stages in.
pub const STAGE_ORDER: [Stage; 11] = [
    Stage::Dust,
    Stage::Crop,
    Stage::Normalize,
    Stage::Invert,
    Stage::Noise,
    Stage::Color,
    Stage::Mixer,
    Stage::Tone,
    Stage::SplitTone,
    Stage::Geometry,
    Stage::Vignette,
];

/// How many stages of [`STAGE_ORDER`] run up to and including noise reduction: Dust, Crop,
/// Normalize, Invert, Noise. They are the only ones that read the scan, its infrared plane, the
/// roll's film base or the render's scale; the stages after them work on the image and the recipe
/// alone (see [`run_late_stage`]), so a picture kept after noise reduction can be finished with
/// nothing else.
pub(crate) const STAGES_UP_TO_NOISE: usize = 5;

const _: () = assert!(matches!(STAGE_ORDER[STAGES_UP_TO_NOISE - 1], Stage::Noise));

/// The film base to normalize with: the recipe's own value when it was sampled for this frame
/// (`Manual`), otherwise the roll's measured base, or the recipe's value if the roll has none.
pub(crate) fn resolve_base(input: &PipelineInput, recipe: &Recipe) -> [f32; 3] {
    match recipe.base.source {
        BaseSource::Manual => recipe.base.rgb,
        BaseSource::Roll => input.film_base.unwrap_or(recipe.base.rgb),
    }
}

/// Runs one stage. Stages that change nothing for this recipe are skipped. `scale` is the size of
/// the image being rendered relative to the full scan (see [`super::render_scale`]); only the
/// noise reduction uses it.
pub(crate) fn run_stage(
    stage: Stage,
    mut image: ImageF32,
    input: &PipelineInput,
    recipe: &Recipe,
    scale: f32,
) -> Result<ImageF32, AppError> {
    match stage {
        Stage::Dust => {
            if let (true, Some(ir)) = (recipe.dust.enabled, &input.ir) {
                // A preview is made from a smaller copy of the scan; the plane follows it.
                let resized;
                let plane = if (ir.width, ir.height) == (image.width, image.height) {
                    ir.as_ref()
                } else {
                    resized = resize_plane(ir, image.width, image.height)?;
                    &resized
                };
                let report = gtp_edit::remove_dust(&mut image, plane, &recipe.dust)?;
                tracing::debug!(
                    masked = report.masked,
                    applied = report.applied,
                    "dust removal"
                );
            }
        }
        Stage::Crop => {
            if recipe.geometry.crop != Crop::default() {
                let crop_only = Geometry {
                    crop: recipe.geometry.crop,
                    ..Geometry::default()
                };
                image = gtp_edit::apply_geometry(&image, &crop_only)?;
            }
        }
        Stage::Normalize => gtp_negative::normalize(&mut image, resolve_base(input, recipe))?,
        Stage::Invert => gtp_negative::invert(&mut image, &recipe.invert)?,
        Stage::Noise => gtp_edit::apply_noise_reduction(&mut image, &recipe.noise, scale)?,
        Stage::Color
        | Stage::Mixer
        | Stage::Tone
        | Stage::SplitTone
        | Stage::Geometry
        | Stage::Vignette => return run_late_stage(stage, image, recipe),
    }
    Ok(image)
}

/// Runs one of the stages after noise reduction (Color to Vignette). They take the image and the
/// recipe and nothing else: not the scan, the infrared plane, the film base or the scale. That is
/// what lets a render be finished from a picture kept after noise reduction. A stage before
/// noise reduction is refused (it needs the input that this function does not have).
pub(crate) fn run_late_stage(
    stage: Stage,
    mut image: ImageF32,
    recipe: &Recipe,
) -> Result<ImageF32, AppError> {
    match stage {
        Stage::Color => gtp_edit::apply_color(&mut image, &recipe.color, &recipe.mono)?,
        Stage::Mixer => {
            if !recipe.mono.enabled {
                gtp_edit::apply_mixer(&mut image, &recipe.color.mixer)?;
            }
        }
        Stage::Tone => gtp_edit::apply_tone(&mut image, &recipe.tone)?,
        Stage::SplitTone => gtp_edit::apply_split_tone(&mut image, &recipe.color.split_tone)?,
        Stage::Geometry => {
            let turn_only = Geometry {
                crop: Crop::default(),
                ..recipe.geometry
            };
            if turn_only != Geometry::default() {
                image = gtp_edit::apply_geometry(&image, &turn_only)?;
            }
        }
        Stage::Vignette => gtp_edit::apply_vignette(&mut image, &recipe.tone.vignette)?,
        Stage::Dust | Stage::Crop | Stage::Normalize | Stage::Invert | Stage::Noise => {
            return Err(AppError::new(
                ErrorCode::Internal,
                "A stage before noise reduction needs the scan.",
            )
            .with_details(format!("{stage:?}")));
        }
    }
    Ok(image)
}
