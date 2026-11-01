//! The fixed stage order (docs/specification.md section 4.2, rules/colour-pipeline.md).

use gtp_core::recipe::{BaseSource, Crop, Geometry};
use gtp_core::{AppError, ImageF32, Recipe};

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
    /// Colour axes, temperature, tint, black and white, saturation, vibrance.
    Color,
    /// Exposure, contrast, highlights, shadows, whites, blacks, dehaze, curves.
    Tone,
    /// Straighten, quarter turns and flips (the crop was applied earlier).
    Geometry,
}

/// The order every render runs the stages in.
pub const STAGE_ORDER: [Stage; 7] = [
    Stage::Dust,
    Stage::Crop,
    Stage::Normalize,
    Stage::Invert,
    Stage::Color,
    Stage::Tone,
    Stage::Geometry,
];

/// The film base to normalize with: the recipe's own value when it was sampled for this frame
/// (`Manual`), otherwise the roll's measured base, or the recipe's value if the roll has none.
pub(crate) fn resolve_base(input: &PipelineInput, recipe: &Recipe) -> [f32; 3] {
    match recipe.base.source {
        BaseSource::Manual => recipe.base.rgb,
        BaseSource::Roll => input.film_base.unwrap_or(recipe.base.rgb),
    }
}

/// Runs one stage. Stages that change nothing for this recipe are skipped.
pub(crate) fn run_stage(
    stage: Stage,
    mut image: ImageF32,
    input: &PipelineInput,
    recipe: &Recipe,
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
        Stage::Color => gtp_edit::apply_color(&mut image, &recipe.color, &recipe.mono)?,
        Stage::Tone => gtp_edit::apply_tone(&mut image, &recipe.tone)?,
        Stage::Geometry => {
            let turn_only = Geometry {
                crop: Crop::default(),
                ..recipe.geometry
            };
            if turn_only != Geometry::default() {
                image = gtp_edit::apply_geometry(&image, &turn_only)?;
            }
        }
    }
    Ok(image)
}
