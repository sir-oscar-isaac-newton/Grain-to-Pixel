//! The render pipeline: one negative and one Recipe in, one linear Rec.2020 positive out.
//!
//! Stages run in the fixed order of [`STAGE_ORDER`]. The crop is applied early as an exact copy, so
//! later stages work on the frame only, and auto levels never see the rebate; straighten, turns
//! and flips come next, so there is still at most one resample; the vignette is last, so it is
//! centred on the final picture. Noise reduction follows the inversion, so it works on the
//! positive before any colour change, at a scale that is the size of the image relative to the full
//! scan (see [`render_scale`]); the pickers, which measure the frame before the colour stage, are
//! unaffected by it, because it is not part of the frames they measure. The colour mixer follows the colour stage (and does nothing in
//! black and white), so the pickers and the automatic balance, which measure the frame before the
//! colour stage, never see it. Split toning follows the tone stage, so it tints the finished
//! tonal result before the geometry. `render` never runs anything automatic: it uses the recipe as
//! given, and [`analyze`] is what fills in the automatic values.

mod analyze;
mod display;
mod proxy;
mod stages;

use std::sync::Arc;

use gtp_core::{AppError, ErrorCode, ImageF32, Recipe};

pub use analyze::{
    analyze, analyze_with, analyze_with_axes, analyze_with_balance, analyze_with_levels,
    analyze_with_look, apply_balance_axes, frame_hint, Analysis, LevelsPercentiles,
};
pub use display::{histogram, to_rgba8_srgb, Histogram};
pub use proxy::{
    build_plane_proxy, build_proxy, resize_plane, resize_to_long_edge, PREVIEW_EDGE, PROXY_EDGE,
};
pub use stages::{Stage, STAGE_ORDER};
/// What a render starts from.
#[derive(Debug, Clone)]
pub struct PipelineInput {
    /// The scan (or a proxy of it): linear RGB, 3 channels.
    pub rgb: Arc<ImageF32>,
    /// The infrared plane (1 channel), if the scan has one and dust removal is wanted. It is the size
    /// of `rgb`; a render at another size resizes it.
    pub ir: Option<Arc<ImageF32>>,
    /// The roll's measured film base, if there is one.
    pub film_base: Option<[f32; 3]>,
    /// The long edge, in pixels, of the full scan that `rgb` was made from, when `rgb` is a proxy;
    /// `None` when `rgb` is the full scan. A render uses it to know its scale.
    pub full_long_edge: Option<u32>,
}

/// How big the result should be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderTarget {
    /// A preview whose long edge is at most `max_edge`: the input is downscaled first unless it is
    /// already that small (pass the frame's proxy for interactive work).
    Preview { max_edge: u32 },
    /// Full resolution, for export.
    Full,
}

/// Renders `input` with `recipe` (clamped first). The result is linear Rec.2020; convert it for
/// display with [`to_rgba8_srgb`].
pub fn render(
    input: &PipelineInput,
    recipe: &Recipe,
    target: RenderTarget,
) -> Result<ImageF32, AppError> {
    render_traced(input, recipe, target, &mut |_| {})
}

/// [`render`], reporting each stage as it starts (used to test the order). It is
/// [`render_up_to_noise`] followed by [`render_from_color`], with the same recipe and input.
pub(crate) fn render_traced(
    input: &PipelineInput,
    recipe: &Recipe,
    target: RenderTarget,
    trace: &mut dyn FnMut(Stage),
) -> Result<ImageF32, AppError> {
    let image = render_up_to_noise(input, recipe, target, trace)?;
    render_from_color(image, recipe, trace)
}

/// The first half of a render: `input` through Dust, Crop, Normalize, Invert and Noise (the stages
/// of `STAGES_UP_TO_NOISE`), with `recipe` clamped first. Everything the stages after it need
/// from the scan is in the picture it returns, so [`render_from_color`] finishes the render from
/// that picture alone. The scale of the render (see [`render_scale`]) is measured here, on the
/// image the stages start with, and only the noise reduction uses it.
pub(crate) fn render_up_to_noise(
    input: &PipelineInput,
    recipe: &Recipe,
    target: RenderTarget,
    trace: &mut dyn FnMut(Stage),
) -> Result<ImageF32, AppError> {
    if input.rgb.channels != 3 {
        return Err(
            AppError::new(ErrorCode::InvalidInput, "Rendering needs a 3-channel scan.")
                .with_details(format!("{} channels", input.rgb.channels)),
        );
    }
    let recipe = recipe.clone().clamped();
    let mut image = match target {
        RenderTarget::Preview { max_edge } => build_proxy(&input.rgb, max_edge)?,
        RenderTarget::Full => (*input.rgb).clone(),
    };
    // Measured before the crop, which comes first: the crop does not change the scale.
    let scale = render_scale(image.width.max(image.height), input);
    for stage in &STAGE_ORDER[..stages::STAGES_UP_TO_NOISE] {
        trace(*stage);
        image = stages::run_stage(*stage, image, input, &recipe, scale)?;
    }
    Ok(image)
}

/// The second half of a render: `image`, as [`render_up_to_noise`] returned it, through Color,
/// Mixer, Tone, SplitTone, Geometry and Vignette, with `recipe` clamped first. These stages read
/// the image and the recipe only (see `stages::run_late_stage`), so a kept copy of that picture
/// gives the same result as the render it came from.
pub(crate) fn render_from_color(
    image: ImageF32,
    recipe: &Recipe,
    trace: &mut dyn FnMut(Stage),
) -> Result<ImageF32, AppError> {
    let recipe = recipe.clone().clamped();
    let mut image = image;
    for stage in &STAGE_ORDER[stages::STAGES_UP_TO_NOISE..] {
        trace(*stage);
        image = stages::run_late_stage(*stage, image, &recipe)?;
    }
    Ok(image)
}

/// The size of the image a render starts from, relative to the full scan: `rendered_long_edge` (the
/// long edge of the uncropped image the stages start with) over the long edge of the full scan.
/// It is 1 for a full render of the scan itself (export), and below 1 for a preview, a proxy, or
/// both. The crop is not part of it, so it is the same for every crop of the same picture.
pub(crate) fn render_scale(rendered_long_edge: u32, input: &PipelineInput) -> f32 {
    let full = input
        .full_long_edge
        .unwrap_or_else(|| input.rgb.width.max(input.rgb.height));
    if full == 0 {
        return 1.0;
    }
    (rendered_long_edge as f32 / full as f32).min(1.0)
}

/// The frame as the Invert stage receives it: `input` after dust removal, the crop and the film
/// base normalization of `recipe` (clamped first), at the size of `input`. This is what the white
/// and black pickers measure.
pub(crate) fn normalized_frame(
    input: &PipelineInput,
    recipe: &Recipe,
) -> Result<ImageF32, AppError> {
    let recipe = recipe.clone().clamped();
    let mut image = (*input.rgb).clone();
    for stage in [Stage::Dust, Stage::Crop, Stage::Normalize] {
        image = stages::run_stage(stage, image, input, &recipe, 1.0)?;
    }
    Ok(image)
}

/// The frame as the Color stage receives it: [`normalized_frame`] followed by the Invert stage of
/// `recipe`. This is what the neutral picker measures.
pub(crate) fn inverted_frame(input: &PipelineInput, recipe: &Recipe) -> Result<ImageF32, AppError> {
    let recipe = recipe.clone().clamped();
    let image = normalized_frame(input, &recipe)?;
    stages::run_stage(Stage::Invert, image, input, &recipe, 1.0)
}

#[cfg(test)]
mod tests {
    use gtp_core::recipe::{BaseSource, Crop, Invert};
    use gtp_core::testing::synthetic::{self, Params, Synthetic};

    use super::stages::resolve_base;
    use super::*;

    fn scan(params: &Params) -> Synthetic {
        synthetic::negative(5, params).unwrap()
    }

    fn input_of(s: &Synthetic, film_base: Option<[f32; 3]>) -> PipelineInput {
        PipelineInput {
            rgb: Arc::new(s.image.clone()),
            ir: None,
            film_base,
            full_long_edge: None,
        }
    }

    /// The recipe that undoes the synthetic model exactly: true crop, base and levels.
    fn true_recipe(s: &Synthetic) -> Recipe {
        let (w, h) = (s.image.width as f32, s.image.height as f32);
        let f = s.truth.frame;
        let mut recipe = Recipe::default();
        recipe.geometry.crop = Crop {
            x: f.x as f32 / w,
            y: f.y as f32 / h,
            w: f.w as f32 / w,
            h: f.h as f32 / h,
        };
        recipe.base.source = BaseSource::Manual;
        recipe.base.rgb = s.truth.base;
        recipe.invert = Invert {
            black: s.truth.black,
            white: s.truth.white,
        };
        recipe
    }

    /// The expected positive inside the frame, as its own image.
    fn expected_frame(s: &Synthetic) -> Vec<f32> {
        let f = s.truth.frame;
        let mut data = Vec::new();
        for y in f.y..f.y + f.h {
            let row = s.truth.expected_positive.row(y).unwrap();
            data.extend_from_slice(&row[f.x as usize * 3..(f.x + f.w) as usize * 3]);
        }
        data
    }

    #[test]
    fn rendering_with_the_true_recipe_recovers_the_synthetic_positive() {
        let s = scan(&Params {
            width: 200,
            height: 150,
            rebate: [16, 10, 24, 12],
            ..Params::default()
        });
        let out = render(&input_of(&s, None), &true_recipe(&s), RenderTarget::Full).unwrap();
        assert_eq!((out.width, out.height), (s.truth.frame.w, s.truth.frame.h));
        for (got, want) in out.data.iter().zip(expected_frame(&s)) {
            assert!((got - want).abs() < 1e-3, "{got} vs {want}");
        }
    }

    fn dusty(specks: u32) -> Synthetic {
        scan(&Params {
            width: 800,
            height: 600,
            rebate: [60, 40, 90, 50],
            dust_specks: specks,
            with_ir: specks > 0,
            ..Params::default()
        })
    }

    fn with_ir(s: &Synthetic) -> PipelineInput {
        PipelineInput {
            ir: s.ir.clone().map(Arc::new),
            ..input_of(s, None)
        }
    }

    /// How many samples of two renders differ by more than a tenth.
    fn wrong(a: &ImageF32, b: &ImageF32) -> usize {
        a.data
            .iter()
            .zip(&b.data)
            .filter(|(x, y)| (*x - *y).abs() > 0.1)
            .count()
    }

    #[test]
    fn the_dust_stage_fills_the_specks_the_infrared_plane_shows() {
        let (dust, clean) = (dusty(6), dusty(0));
        let recipe = true_recipe(&dust);
        let want = render(&input_of(&clean, None), &recipe, RenderTarget::Full).unwrap();
        let without = render(&input_of(&dust, None), &recipe, RenderTarget::Full).unwrap();
        let fixed = render(&with_ir(&dust), &recipe, RenderTarget::Full).unwrap();
        let (before, after) = (wrong(&without, &want), wrong(&fixed, &want));
        assert!(before > 30, "the specks show");
        assert!(
            after * 4 < before,
            "{after} samples wrong after, {before} before"
        );
    }

    #[test]
    fn the_dust_stage_is_skipped_when_disabled_and_without_a_plane() {
        let dust = dusty(6);
        let mut recipe = true_recipe(&dust);
        let plain = render(&input_of(&dust, None), &recipe, RenderTarget::Full).unwrap();
        recipe.dust.enabled = false;
        let off = render(&with_ir(&dust), &recipe, RenderTarget::Full).unwrap();
        assert_eq!(off, plain);
        // A scan with no dust is unchanged by the stage.
        let clean = dusty(0);
        let recipe = true_recipe(&clean);
        let plain = render(&input_of(&clean, None), &recipe, RenderTarget::Full).unwrap();
        let plane = ImageF32::new(800, 600, 1, vec![1.0; 800 * 600]).unwrap();
        let input = PipelineInput {
            ir: Some(Arc::new(plane)),
            ..input_of(&clean, None)
        };
        assert_eq!(render(&input, &recipe, RenderTarget::Full).unwrap(), plain);
    }

    #[test]
    fn a_preview_resizes_the_infrared_plane_to_the_scaled_scan() {
        let dust = dusty(6);
        let recipe = true_recipe(&dust);
        let small = render(
            &with_ir(&dust),
            &recipe,
            RenderTarget::Preview { max_edge: 100 },
        );
        let small = small.unwrap();
        assert!(small.width > 0 && small.height > 0);
        assert!(small.data.iter().all(|v| v.is_finite()));
    }
    #[test]
    fn one_stop_of_exposure_doubles_the_rendered_positive() {
        let s = scan(&Params {
            width: 120,
            height: 80,
            rebate: [10; 4],
            ..Params::default()
        });
        let mut recipe = true_recipe(&s);
        recipe.tone.exposure_ev = 1.0;
        let out = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        for (got, want) in out.data.iter().zip(expected_frame(&s)) {
            assert!((got - 2.0 * want).abs() < 2e-3, "{got} vs {}", 2.0 * want);
        }
    }

    #[test]
    fn the_stages_run_in_the_fixed_order() {
        let s = scan(&Params::default());
        let mut seen = Vec::new();
        render_traced(
            &input_of(&s, None),
            &true_recipe(&s),
            RenderTarget::Full,
            &mut |stage| {
                seen.push(stage);
            },
        )
        .unwrap();
        assert_eq!(seen, STAGE_ORDER);
        assert_eq!(
            STAGE_ORDER,
            [
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
                Stage::Vignette
            ]
        );
    }

    #[test]
    fn the_film_base_comes_from_the_recipe_or_the_roll() {
        let s = scan(&Params::default());
        let roll = [0.7, 0.5, 0.3];
        let mut recipe = Recipe::default();
        recipe.base.rgb = [0.9, 0.6, 0.4];

        recipe.base.source = BaseSource::Manual;
        assert_eq!(
            resolve_base(&input_of(&s, Some(roll)), &recipe),
            [0.9, 0.6, 0.4]
        );
        recipe.base.source = BaseSource::Roll;
        assert_eq!(resolve_base(&input_of(&s, Some(roll)), &recipe), roll);
        assert_eq!(resolve_base(&input_of(&s, None), &recipe), [0.9, 0.6, 0.4]);
    }

    #[test]
    fn a_preview_is_the_full_render_of_the_proxy() {
        let s = scan(&Params {
            width: 400,
            height: 300,
            rebate: [30; 4],
            ..Params::default()
        });
        let recipe = true_recipe(&s);
        let preview = render(
            &input_of(&s, None),
            &recipe,
            RenderTarget::Preview { max_edge: 160 },
        )
        .unwrap();
        let proxy = PipelineInput {
            rgb: Arc::new(build_proxy(&s.image, 160).unwrap()),
            ir: None,
            film_base: None,
            full_long_edge: None,
        };
        assert_eq!(
            preview,
            render(&proxy, &recipe, RenderTarget::Full).unwrap()
        );
        assert!(preview.width.max(preview.height) <= 160);
    }

    #[test]
    fn turns_and_straightening_happen_after_the_crop() {
        let s = scan(&Params {
            width: 200,
            height: 150,
            rebate: [16; 4],
            ..Params::default()
        });
        let mut recipe = true_recipe(&s);
        recipe.geometry.rot90 = 1;
        let turned = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        assert_eq!(
            (turned.width, turned.height),
            (s.truth.frame.h, s.truth.frame.w)
        );

        recipe.geometry.rot90 = 0;
        recipe.geometry.angle_deg = 3.0;
        let straight = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        assert!(
            straight.width < s.truth.frame.w,
            "straightening the frame costs a few percent"
        );
    }

    /// A flat scan whose positive is a uniform grey, so only the vignette makes it uneven.
    fn flat_scan(width: u32, height: u32) -> Synthetic {
        scan(&Params {
            width,
            height,
            rebate: [0; 4],
            ..Params::default()
        })
    }

    fn at(image: &ImageF32, fx: f32, fy: f32) -> [f32; 3] {
        let x = ((fx * image.width as f32) as u32).min(image.width - 1);
        let y = ((fy * image.height as f32) as u32).min(image.height - 1);
        let i = (y * image.width + x) as usize * 3;
        [image.data[i], image.data[i + 1], image.data[i + 2]]
    }

    fn at_px(image: &ImageF32, x: u32, y: u32) -> [f32; 3] {
        let i = (y * image.width + x) as usize * 3;
        [image.data[i], image.data[i + 1], image.data[i + 2]]
    }

    #[test]
    fn a_vignette_of_zero_changes_nothing() {
        let s = scan(&Params {
            width: 120,
            height: 80,
            rebate: [10; 4],
            ..Params::default()
        });
        let recipe = true_recipe(&s);
        let plain = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        let mut with_midpoint = recipe.clone();
        with_midpoint.tone.vignette.midpoint = 10.0;
        assert_eq!(
            render(&input_of(&s, None), &with_midpoint, RenderTarget::Full).unwrap(),
            plain
        );
        let mut dark = recipe;
        dark.tone.vignette.amount = -80.0;
        let darker = render(&input_of(&s, None), &dark, RenderTarget::Full).unwrap();
        assert_ne!(darker, plain);
        assert_eq!(
            at(&darker, 0.5, 0.5),
            at(&plain, 0.5, 0.5),
            "the centre stays"
        );
    }

    #[test]
    fn the_vignette_is_centred_on_the_cropped_and_turned_picture() {
        let s = scan(&Params {
            width: 200,
            height: 150,
            rebate: [16, 10, 24, 12],
            ..Params::default()
        });
        let mut recipe = true_recipe(&s);
        recipe.geometry.rot90 = 1;
        let plain = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        recipe.tone.vignette.amount = -100.0;
        recipe.tone.vignette.midpoint = 20.0;
        let out = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        assert_eq!((out.width, out.height), (plain.width, plain.height));
        // It is the vignette of the finished (cropped and turned) picture, centred on it.
        let mut want = plain.clone();
        gtp_edit::apply_vignette(&mut want, &recipe.tone.vignette).unwrap();
        assert_eq!(out, want);
        // Every corner of the output is darkened by the same factor (the picture there may not be
        // black: look for the first non-black pixel along the diagonal from each corner).
        let (w, h) = (out.width, out.height);
        let factor = |x: u32, y: u32| {
            let (a, b) = (at_px(&out, x, y), at_px(&plain, x, y));
            (a[1] > 0.0).then(|| a[1] / b[1])
        };
        let corners = [
            factor(0, 0),
            factor(w - 1, 0),
            factor(0, h - 1),
            factor(w - 1, h - 1),
        ];
        for g in corners.into_iter().flatten() {
            assert!((g - 0.25).abs() < 0.03, "{g}");
        }
        assert!(corners.iter().flatten().count() >= 2, "{corners:?}");
    }

    #[test]
    fn a_preview_and_a_full_render_agree_at_the_same_relative_positions() {
        let s = flat_scan(800, 400);
        let mut recipe = Recipe::default();
        recipe.base.source = BaseSource::Manual;
        recipe.base.rgb = s.truth.base;
        recipe.invert = Invert {
            black: s.truth.black,
            white: s.truth.white,
        };
        let plain = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        let target = RenderTarget::Preview { max_edge: 200 };
        let plain_preview = render(&input_of(&s, None), &recipe, target).unwrap();
        recipe.tone.vignette.amount = -70.0;
        recipe.tone.vignette.midpoint = 30.0;
        let full = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        let preview = render(&input_of(&s, None), &recipe, target).unwrap();
        assert_eq!((preview.width, preview.height), (200, 100));
        for (fx, fy) in [(0.1, 0.2), (0.5, 0.5), (0.8, 0.9), (0.95, 0.5)] {
            // The same gain, measured against the unvignetted full render.
            let g_full = at(&full, fx, fy)[1] / at(&plain, fx, fy)[1];
            let g_preview = at(&preview, fx, fy)[1] / at(&plain_preview, fx, fy)[1];
            assert!((g_full - g_preview).abs() < 0.03, "({fx}, {fy})");
        }
    }

    fn luma_of(p: [f32; 3]) -> f32 {
        let [wr, wg, wb] = gtp_core::recipe::REC2020_LUMA;
        wr * p[0] + wg * p[1] + wb * p[2]
    }

    #[test]
    fn split_toning_with_both_strengths_at_zero_changes_nothing() {
        let s = scan(&Params {
            width: 120,
            height: 80,
            rebate: [10; 4],
            ..Params::default()
        });
        let recipe = true_recipe(&s);
        let plain = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        let mut other = recipe.clone();
        other.color.split_tone.highlight_hue = 300.0;
        other.color.split_tone.shadow_hue = 10.0;
        other.color.split_tone.balance = 60.0;
        assert_eq!(
            render(&input_of(&s, None), &other, RenderTarget::Full).unwrap(),
            plain
        );
        other.color.split_tone.shadow_strength = 80.0;
        let toned = render(&input_of(&s, None), &other, RenderTarget::Full).unwrap();
        assert_ne!(toned, plain);
        assert_eq!((toned.width, toned.height), (plain.width, plain.height));
    }

    #[test]
    fn split_toning_runs_after_the_tone_stage_and_keeps_the_luma() {
        let s = scan(&Params {
            width: 120,
            height: 80,
            rebate: [10; 4],
            ..Params::default()
        });
        let mut recipe = true_recipe(&s);
        recipe.tone.exposure_ev = 0.5;
        recipe.tone.contrast = 30.0;
        let plain = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        recipe.color.split_tone.highlight_strength = 70.0;
        recipe.color.split_tone.shadow_strength = 60.0;
        let toned = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        // Applying the stage to the render without it gives the same picture: it comes after the
        // tone stage and does not depend on anything earlier.
        let mut want = plain.clone();
        gtp_edit::apply_split_tone(&mut want, &recipe.color.split_tone).unwrap();
        assert_eq!(toned, want);
        for (a, b) in plain.data.chunks(3).zip(toned.data.chunks(3)) {
            let (ya, yb) = (luma_of([a[0], a[1], a[2]]), luma_of([b[0], b[1], b[2]]));
            assert!((ya - yb).abs() <= 1e-4 * ya.max(1e-3), "{ya} vs {yb}");
        }
    }

    #[test]
    fn split_toning_tints_a_black_and_white_picture_without_changing_its_luma() {
        let s = scan(&Params {
            width: 120,
            height: 80,
            rebate: [10; 4],
            ..Params::default()
        });
        let mut recipe = true_recipe(&s);
        recipe.mono.enabled = true;
        let plain = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        assert!(
            plain.data.chunks(3).all(|p| p[0] == p[1] && p[1] == p[2]),
            "mono is neutral without the tint"
        );
        recipe.color.split_tone.highlight_hue = 30.0;
        recipe.color.split_tone.highlight_strength = 80.0;
        recipe.color.split_tone.shadow_hue = 220.0;
        recipe.color.split_tone.shadow_strength = 80.0;
        let toned = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        let coloured = toned
            .data
            .chunks(3)
            .filter(|p| (p[0] - p[2]).abs() > 1e-3 * p[1].max(1e-3))
            .count();
        assert!(coloured > toned.data.len() / 3 / 2, "{coloured}");
        for (a, b) in plain.data.chunks(3).zip(toned.data.chunks(3)) {
            let (ya, yb) = (luma_of([a[0], a[1], a[2]]), luma_of([b[0], b[1], b[2]]));
            assert!((ya - yb).abs() <= 1e-4 * ya.max(1e-3), "{ya} vs {yb}");
        }
    }

    #[test]
    fn split_toning_agrees_between_a_preview_and_a_full_render() {
        let s = flat_scan(800, 400);
        let mut recipe = Recipe::default();
        recipe.base.source = BaseSource::Manual;
        recipe.base.rgb = s.truth.base;
        recipe.invert = Invert {
            black: s.truth.black,
            white: s.truth.white,
        };
        recipe.color.split_tone.highlight_hue = 20.0;
        recipe.color.split_tone.highlight_strength = 90.0;
        recipe.color.split_tone.shadow_hue = 230.0;
        recipe.color.split_tone.shadow_strength = 90.0;
        recipe.color.split_tone.balance = 15.0;
        let full = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        let target = RenderTarget::Preview { max_edge: 200 };
        let preview = render(&input_of(&s, None), &recipe, target).unwrap();
        assert_eq!((preview.width, preview.height), (200, 100));
        for (fx, fy) in [(0.1, 0.2), (0.5, 0.5), (0.8, 0.9)] {
            let (a, b) = (at(&full, fx, fy), at(&preview, fx, fy));
            for i in 0..3 {
                assert!((a[i] - b[i]).abs() <= 0.05 * a[i].max(0.05), "{a:?} {b:?}");
            }
        }
    }

    /// A mixer that changes every colourful pixel: every band desaturated, shifted and lightened.
    fn loud_mixer() -> gtp_core::recipe::Mixer {
        gtp_core::recipe::Mixer {
            hue: [60.0; 8],
            saturation: [-70.0; 8],
            lightness: [40.0; 8],
        }
    }

    /// The render of `recipe` through every stage but the mixer, run one by one.
    fn render_without_the_mixer(s: &Synthetic, recipe: &Recipe) -> ImageF32 {
        let input = input_of(s, None);
        let recipe = recipe.clone().clamped();
        let mut image = (*input.rgb).clone();
        for stage in STAGE_ORDER {
            if stage != Stage::Mixer {
                image = stages::run_stage(stage, image, &input, &recipe, 1.0).unwrap();
            }
        }
        image
    }

    #[test]
    fn a_colour_mixer_of_zeros_changes_nothing() {
        let s = scan(&Params {
            width: 120,
            height: 80,
            rebate: [10; 4],
            ..Params::default()
        });
        let recipe = true_recipe(&s);
        assert_eq!(recipe.color.mixer, gtp_core::recipe::Mixer::default());
        let plain = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        assert_eq!(plain, render_without_the_mixer(&s, &recipe));
    }

    #[test]
    fn the_colour_mixer_runs_between_the_colour_and_the_tone_stage() {
        let s = scan(&Params {
            width: 120,
            height: 80,
            rebate: [10; 4],
            ..Params::default()
        });
        let mut recipe = true_recipe(&s);
        recipe.color.red_cyan = 20.0;
        recipe.tone.exposure_ev = 0.5;
        recipe.tone.contrast = 30.0;
        recipe.color.mixer = loud_mixer();
        let input = input_of(&s, None);
        let mixed = render(&input, &recipe, RenderTarget::Full).unwrap();
        assert_ne!(mixed, render_without_the_mixer(&s, &recipe));
        // The same stages by hand, with the mixer applied straight after the colour stage.
        let clamped = recipe.clone().clamped();
        let mut image = (*input.rgb).clone();
        for stage in STAGE_ORDER {
            if stage == Stage::Mixer {
                gtp_edit::apply_mixer(&mut image, &clamped.color.mixer).unwrap();
            } else {
                image = stages::run_stage(stage, image, &input, &clamped, 1.0).unwrap();
            }
        }
        assert_eq!(mixed, image);
    }

    #[test]
    fn the_colour_mixer_does_nothing_in_black_and_white() {
        let s = scan(&Params {
            width: 120,
            height: 80,
            rebate: [10; 4],
            ..Params::default()
        });
        let mut recipe = true_recipe(&s);
        recipe.mono.enabled = true;
        let plain = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        recipe.color.mixer = loud_mixer();
        let mixed = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        assert_eq!(mixed, plain);
        recipe.mono.enabled = false;
        let colour = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        assert_ne!(colour, render_without_the_mixer(&s, &recipe));
    }

    #[test]
    fn the_frames_the_pickers_measure_do_not_see_the_colour_mixer() {
        let s = scan(&Params::default());
        let input = input_of(&s, None);
        let mut recipe = true_recipe(&s);
        let normalized = normalized_frame(&input, &recipe).unwrap();
        let inverted = inverted_frame(&input, &recipe).unwrap();
        recipe.color.mixer = loud_mixer();
        assert_eq!(normalized_frame(&input, &recipe).unwrap(), normalized);
        assert_eq!(inverted_frame(&input, &recipe).unwrap(), inverted);
    }

    #[test]
    fn the_colour_mixer_agrees_between_a_preview_and_a_full_render() {
        let s = flat_scan(800, 400);
        let mut recipe = Recipe::default();
        recipe.base.source = BaseSource::Manual;
        recipe.base.rgb = s.truth.base;
        recipe.invert = Invert {
            black: s.truth.black,
            white: s.truth.white,
        };
        recipe.color.mixer = loud_mixer();
        let plain_recipe = Recipe {
            color: gtp_core::recipe::Color {
                mixer: Default::default(),
                ..recipe.color
            },
            ..recipe.clone()
        };
        let target = RenderTarget::Preview { max_edge: 200 };
        let full = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        let plain = render(&input_of(&s, None), &plain_recipe, RenderTarget::Full).unwrap();
        assert_ne!(full, plain, "the mixer changes this picture");
        let preview = render(&input_of(&s, None), &recipe, target).unwrap();
        assert_eq!((preview.width, preview.height), (200, 100));
        for (fx, fy) in [(0.1, 0.2), (0.5, 0.5), (0.8, 0.9)] {
            let (a, b) = (at(&full, fx, fy), at(&preview, fx, fy));
            for i in 0..3 {
                assert!((a[i] - b[i]).abs() <= 0.05 * a[i].max(0.05), "{a:?} {b:?}");
            }
        }
    }

    fn loud_noise() -> gtp_core::recipe::Noise {
        gtp_core::recipe::Noise {
            luminance: 100.0,
            color: 100.0,
        }
    }

    /// A noisy scan, so that noise reduction has something to do.
    fn noisy_scan(width: u32, height: u32) -> Synthetic {
        scan(&Params {
            width,
            height,
            rebate: [0; 4],
            noise: 0.08,
            ..Params::default()
        })
    }

    /// The render of `recipe` through every stage but `skipped`, run one by one.
    fn render_without(s: &Synthetic, recipe: &Recipe, skipped: Stage) -> ImageF32 {
        let input = input_of(s, None);
        let recipe = recipe.clone().clamped();
        let mut image = (*input.rgb).clone();
        for stage in STAGE_ORDER {
            if stage != skipped {
                image = stages::run_stage(stage, image, &input, &recipe, 1.0).unwrap();
            }
        }
        image
    }

    #[test]
    fn noise_reduction_of_zeros_changes_nothing() {
        let s = noisy_scan(120, 80);
        let recipe = true_recipe(&s);
        assert_eq!(recipe.noise, gtp_core::recipe::Noise::default());
        let plain = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        assert_eq!(plain, render_without(&s, &recipe, Stage::Noise));
    }

    #[test]
    fn noise_reduction_runs_between_the_inversion_and_the_colour_stage() {
        let s = noisy_scan(120, 80);
        let mut recipe = true_recipe(&s);
        recipe.color.red_cyan = 20.0;
        recipe.tone.exposure_ev = 0.5;
        recipe.noise = loud_noise();
        let input = input_of(&s, None);
        let reduced = render(&input, &recipe, RenderTarget::Full).unwrap();
        assert_ne!(reduced, render_without(&s, &recipe, Stage::Noise));
        // The same stages by hand, with the reduction applied straight after the inversion.
        let clamped = recipe.clone().clamped();
        let mut image = (*input.rgb).clone();
        for stage in STAGE_ORDER {
            if stage == Stage::Noise {
                gtp_edit::apply_noise_reduction(&mut image, &clamped.noise, 1.0).unwrap();
            } else {
                image = stages::run_stage(stage, image, &input, &clamped, 1.0).unwrap();
            }
        }
        assert_eq!(reduced, image);
    }

    #[test]
    fn the_frames_the_pickers_measure_do_not_see_noise_reduction() {
        let s = noisy_scan(160, 100);
        let input = input_of(&s, None);
        let mut recipe = true_recipe(&s);
        let normalized = normalized_frame(&input, &recipe).unwrap();
        let inverted = inverted_frame(&input, &recipe).unwrap();
        recipe.noise = loud_noise();
        assert_eq!(normalized_frame(&input, &recipe).unwrap(), normalized);
        assert_eq!(inverted_frame(&input, &recipe).unwrap(), inverted);
    }

    #[test]
    fn noise_reduction_agrees_between_a_preview_and_a_full_render() {
        let s = noisy_scan(800, 400);
        let mut recipe = Recipe::default();
        recipe.base.source = BaseSource::Manual;
        recipe.base.rgb = s.truth.base;
        recipe.invert = Invert {
            black: s.truth.black,
            white: s.truth.white,
        };
        let mut plain = recipe.clone();
        recipe.noise = loud_noise();
        plain.noise = Default::default();
        let target = RenderTarget::Preview { max_edge: 200 };
        let full = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap();
        let unreduced = render(&input_of(&s, None), &plain, RenderTarget::Full).unwrap();
        assert_ne!(full, unreduced, "the reduction changes this picture");
        let preview = render(&input_of(&s, None), &recipe, target).unwrap();
        assert_eq!((preview.width, preview.height), (200, 100));
        // Averaged over blocks of 4 by 4 full-size pixels, so the remaining grain does not count:
        // within 5 percent (and at least 0.02) of the full-size result.
        for (fx, fy) in [(0.1, 0.2), (0.5, 0.5), (0.8, 0.9)] {
            let (px, py) = (
                (fx * preview.width as f32) as u32,
                (fy * preview.height as f32) as u32,
            );
            let b = at_px(&preview, px, py);
            let mut a = [0.0_f32; 3];
            for (dx, dy) in (0..4).flat_map(|dy| (0..4).map(move |dx| (dx, dy))) {
                let p = at_px(&full, px * 4 + dx, py * 4 + dy);
                (0..3).for_each(|i| a[i] += p[i] / 16.0);
            }
            for i in 0..3 {
                assert!((a[i] - b[i]).abs() <= 0.05 * a[i].max(0.4), "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn the_scale_is_one_at_export_and_below_one_for_a_preview() {
        let s = flat_scan(800, 400);
        // Export: the full scan, rendered in full.
        let full = input_of(&s, None);
        assert_eq!(render_scale(800, &full), 1.0);
        // A preview made from the full scan itself.
        assert_eq!(render_scale(200, &full), 0.25);
        // A proxy of 1000 pixels from a scan of 4000: a full render of it, and a smaller preview.
        let proxy = PipelineInput {
            full_long_edge: Some(4000),
            ..full
        };
        assert_eq!(render_scale(800, &proxy), 0.2);
        assert_eq!(render_scale(400, &proxy), 0.1);
        // A proxy that is as big as the scan, or a wrong size, never gives more than 1.
        assert_eq!(
            render_scale(
                800,
                &PipelineInput {
                    full_long_edge: Some(100),
                    ..proxy.clone()
                }
            ),
            1.0
        );
        assert_eq!(
            render_scale(
                800,
                &PipelineInput {
                    full_long_edge: Some(0),
                    ..proxy
                }
            ),
            1.0
        );
    }

    #[test]
    fn a_render_of_a_proxy_reduces_noise_at_the_scale_of_the_scan() {
        let s = noisy_scan(400, 200);
        let mut recipe = true_recipe(&s);
        recipe.noise = loud_noise();
        // The same pixels, said to be a proxy of a scan twice as long: the scale is 0.5.
        let proxy = PipelineInput {
            full_long_edge: Some(800),
            ..input_of(&s, None)
        };
        let got = render(&proxy, &recipe, RenderTarget::Full).unwrap();
        let clamped = recipe.clone().clamped();
        let mut want = (*proxy.rgb).clone();
        for stage in STAGE_ORDER {
            if stage == Stage::Noise {
                gtp_edit::apply_noise_reduction(&mut want, &clamped.noise, 0.5).unwrap();
            } else {
                want = stages::run_stage(stage, want, &proxy, &clamped, 0.5).unwrap();
            }
        }
        assert_eq!(got, want);
        assert_ne!(
            got,
            render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap()
        );
    }

    /// A small deterministic generator, so the recipes of a test are the same on every run.
    struct Lcg(u64);

    impl Lcg {
        /// A value from 0 to 1.
        fn unit(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 40) as f32 / (1u64 << 24) as f32
        }

        fn range(&mut self, low: f32, high: f32) -> f32 {
            low + (high - low) * self.unit()
        }

        fn chance(&mut self, p: f32) -> bool {
            self.unit() < p
        }
    }

    /// A recipe with every tool set to something, from the true one for `s`: a part of the time a
    /// crop inside the frame, a base from the roll or by hand, noise reduction, dust removal, a
    /// straighten, turns and flips, and every later tool.
    fn random_recipe(rng: &mut Lcg, s: &Synthetic) -> Recipe {
        let mut recipe = true_recipe(s);
        if rng.chance(0.5) {
            let x = rng.range(0.0, 0.2);
            let y = rng.range(0.0, 0.2);
            recipe.geometry.crop = Crop {
                x,
                y,
                w: rng.range(0.5, 0.8),
                h: rng.range(0.5, 0.8),
            };
        }
        if rng.chance(0.5) {
            recipe.base.source = BaseSource::Roll;
        }
        for v in recipe
            .invert
            .white
            .iter_mut()
            .chain(&mut recipe.invert.black)
        {
            *v *= rng.range(0.9, 1.1);
        }
        recipe.dust.enabled = rng.chance(0.7);
        recipe.dust.threshold = rng.range(0.2, 0.8);
        recipe.dust.grow_px = (rng.unit() * 4.0) as u32;
        if rng.chance(0.7) {
            recipe.noise.luminance = rng.range(0.0, 100.0);
            recipe.noise.color = rng.range(0.0, 100.0);
        }
        let color = &mut recipe.color;
        for v in [
            &mut color.red_cyan,
            &mut color.green_magenta,
            &mut color.blue_yellow,
            &mut color.temperature,
            &mut color.tint,
            &mut color.vibrance,
        ] {
            *v = rng.range(-40.0, 40.0);
        }
        color.saturation = rng.range(0.6, 1.5);
        recipe.mono.enabled = rng.chance(0.25);
        for band in 0..8 {
            color.mixer.hue[band] = rng.range(-50.0, 50.0);
            color.mixer.saturation[band] = rng.range(-50.0, 50.0);
            color.mixer.lightness[band] = rng.range(-50.0, 50.0);
        }
        let split = &mut color.split_tone;
        split.highlight_hue = rng.range(0.0, 360.0);
        split.highlight_strength = rng.range(0.0, 60.0);
        split.shadow_hue = rng.range(0.0, 360.0);
        split.shadow_strength = rng.range(0.0, 60.0);
        split.balance = rng.range(-50.0, 50.0);
        let tone = &mut recipe.tone;
        tone.exposure_ev = rng.range(-0.5, 0.5);
        tone.contrast = rng.range(-40.0, 40.0);
        tone.highlights = rng.range(-40.0, 40.0);
        tone.shadows = rng.range(-40.0, 40.0);
        tone.whites = rng.range(-20.0, 20.0);
        tone.blacks = rng.range(-20.0, 20.0);
        tone.dehaze = rng.range(-20.0, 20.0);
        if rng.chance(0.5) {
            tone.curve.master = vec![[0.0, 0.0], [0.3, rng.range(0.2, 0.4)], [1.0, 1.0]];
            tone.curve.g = vec![[0.0, 0.0], [0.6, rng.range(0.5, 0.7)], [1.0, 1.0]];
        }
        tone.vignette.amount = rng.range(-60.0, 40.0);
        tone.vignette.midpoint = rng.range(20.0, 80.0);
        if rng.chance(0.6) {
            recipe.geometry.angle_deg = rng.range(-3.0, 3.0);
        }
        recipe.geometry.rot90 = (rng.unit() * 4.0) as i32;
        recipe.geometry.flip_h = rng.chance(0.5);
        recipe.geometry.flip_v = rng.chance(0.5);
        recipe
    }

    /// The render as it was before it was split in two: every stage of `STAGE_ORDER` in one loop.
    fn render_in_one_loop(
        input: &PipelineInput,
        recipe: &Recipe,
        target: RenderTarget,
    ) -> ImageF32 {
        let recipe = recipe.clone().clamped();
        let mut image = match target {
            RenderTarget::Preview { max_edge } => build_proxy(&input.rgb, max_edge).unwrap(),
            RenderTarget::Full => (*input.rgb).clone(),
        };
        let scale = render_scale(image.width.max(image.height), input);
        for stage in STAGE_ORDER {
            image = stages::run_stage(stage, image, input, &recipe, scale).unwrap();
        }
        image
    }

    #[test]
    fn a_render_is_the_stages_up_to_noise_followed_by_the_stages_from_color() {
        let s = scan(&Params {
            width: 160,
            height: 120,
            rebate: [10; 4],
            noise: 0.05,
            dust_specks: 4,
            with_ir: true,
            ..Params::default()
        });
        let mut rng = Lcg(7);
        // A scan, a proxy said to come from a scan twice as long (scale below 1), and the same
        // scan with a film base, so the base from the roll counts too.
        let inputs = [
            with_ir(&s),
            PipelineInput {
                full_long_edge: Some(320),
                ..with_ir(&s)
            },
            PipelineInput {
                film_base: Some([0.8, 0.6, 0.4]),
                ..with_ir(&s)
            },
        ];
        let targets = [RenderTarget::Full, RenderTarget::Preview { max_edge: 64 }];
        for round in 0..6 {
            let recipe = random_recipe(&mut rng, &s);
            for input in &inputs {
                for target in targets {
                    let whole = render(input, &recipe, target).unwrap();
                    let (mut first, mut second) = (Vec::new(), Vec::new());
                    let kept = render_up_to_noise(input, &recipe, target, &mut |st| first.push(st))
                        .unwrap();
                    let split =
                        render_from_color(kept, &recipe, &mut |st| second.push(st)).unwrap();
                    assert_eq!(split, whole, "round {round}, {target:?}");
                    assert_eq!(first, STAGE_ORDER[..5], "the stages up to noise");
                    assert_eq!(second, STAGE_ORDER[5..], "the stages from colour");
                    assert_eq!(
                        whole,
                        render_in_one_loop(input, &recipe, target),
                        "round {round}, {target:?}: the same as one loop over every stage"
                    );
                }
            }
        }
    }

    #[test]
    fn the_stages_from_color_work_from_a_copy_of_the_picture_after_noise_reduction() {
        // What the detail view relies on: the picture after noise reduction can be copied, and
        // finishing the copy gives what finishing the original gives, for every later edit.
        let s = noisy_scan(160, 100);
        let mut rng = Lcg(11);
        let input = input_of(&s, None);
        let mut base = true_recipe(&s);
        base.noise = loud_noise();
        let kept = render_up_to_noise(&input, &base, RenderTarget::Full, &mut |_| {}).unwrap();
        for _ in 0..4 {
            // The same first five stages' parts, every later part changed.
            let mut recipe = random_recipe(&mut rng, &s);
            recipe.dust = base.dust;
            recipe.geometry.crop = base.geometry.crop;
            recipe.base = base.base;
            recipe.invert = base.invert;
            recipe.noise = base.noise;
            let from_kept = render_from_color(kept.clone(), &recipe, &mut |_| {}).unwrap();
            assert_eq!(
                from_kept,
                render(&input, &recipe, RenderTarget::Full).unwrap()
            );
        }
    }

    #[test]
    fn the_stages_after_noise_reduction_refuse_a_stage_that_needs_the_scan() {
        let image = ImageF32::zeros(4, 4, 3).unwrap();
        for stage in &STAGE_ORDER[..5] {
            let err =
                stages::run_late_stage(*stage, image.clone(), &Recipe::default()).unwrap_err();
            assert_eq!(err.code, ErrorCode::Internal);
        }
        for stage in &STAGE_ORDER[5..] {
            assert!(stages::run_late_stage(*stage, image.clone(), &Recipe::default()).is_ok());
        }
    }

    #[test]
    fn bad_input_is_an_error() {
        let gray = PipelineInput {
            rgb: Arc::new(ImageF32::zeros(4, 4, 1).unwrap()),
            ir: None,
            film_base: None,
            full_long_edge: None,
        };
        let err = render(&gray, &Recipe::default(), RenderTarget::Full).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);

        let s = scan(&Params::default());
        let mut recipe = Recipe::default();
        recipe.tone.curve.master = vec![[0.5, 0.5]];
        let err = render(&input_of(&s, None), &recipe, RenderTarget::Full).unwrap_err();
        assert_eq!(
            err.code,
            ErrorCode::InvalidInput,
            "an invalid curve reaches the caller"
        );
    }

    /// Where the time of a 1600 px preview from a 2048 px proxy goes, with a typical edit, for a
    /// 3:2 frame (35 mm) and a square one (6x6): the downscale, each stage, and the conversion to
    /// 8-bit sRGB. The budget for the whole is 100 ms (docs/specification.md section 11). Run with
    /// `cargo test -p gtp-app --release -- --ignored --nocapture time_a_1600_px_preview`.
    #[test]
    #[ignore = "timing run, not a check"]
    fn time_a_1600_px_preview() {
        use std::collections::BTreeMap;
        use std::time::{Duration, Instant};

        for (label, width, height) in [("3:2", 2048, 1365), ("square", 2048, 2048)] {
            let s = scan(&Params {
                width,
                height,
                rebate: [60; 4],
                ..Params::default()
            });
            let mut recipe = true_recipe(&s);
            recipe.tone.exposure_ev = 0.3;
            recipe.tone.contrast = 20.0;
            recipe.tone.shadows = 20.0;
            recipe.color.vibrance = 15.0;
            recipe.geometry.angle_deg = 1.0;
            let input = input_of(&s, None);
            let target = RenderTarget::Preview {
                max_edge: PREVIEW_EDGE,
            };
            for _ in 0..3 {
                render(&input, &recipe, target).unwrap();
            }

            let runs: u32 = 20;
            let mut stages = BTreeMap::<String, Duration>::new();
            let (mut downscale, mut convert, mut whole) =
                (Duration::ZERO, Duration::ZERO, Duration::ZERO);
            for _ in 0..runs {
                let started = Instant::now();
                build_proxy(&input.rgb, PREVIEW_EDGE).unwrap();
                downscale += started.elapsed();

                let mut marks = Vec::new();
                let started = Instant::now();
                let image = render_traced(&input, &recipe, target, &mut |stage| {
                    marks.push((stage, Instant::now()));
                })
                .unwrap();
                let ended = Instant::now();
                whole += ended - started;
                for (n, (stage, at)) in marks.iter().enumerate() {
                    let until = marks.get(n + 1).map_or(ended, |next| next.1);
                    *stages.entry(format!("{stage:?}")).or_default() += until - *at;
                }
                let started = Instant::now();
                to_rgba8_srgb(&image).unwrap();
                convert += started.elapsed();
            }
            eprintln!(
                "{label} {width}x{height}: render {:?}, to 8-bit sRGB {:?}, together {:?} (the downscale alone: {:?})",
                whole / runs,
                convert / runs,
                (whole + convert) / runs,
                downscale / runs
            );
            for (name, total) in stages {
                eprintln!("    {name:<10} {:?}", total / runs);
            }
        }
    }

    /// Where the time of a full-size detail render goes (issue P-31, phase 1: profile only): the
    /// decode of the scan and its infrared plane from files in a library (`read_full`), the copy
    /// the render starts with, each stage of a 6000 x 4000 frame with every tool on, the output
    /// sharpening and the conversion to 8-bit sRGB, on rayon pools of 6 and 12 threads (median of
    /// 5 runs, after one that is not counted). The stages are timed with the render's own trace
    /// hook, so the numbers are those of `render_full`. Run with
    /// `cargo test --release -p gtp-app time_a_full_size_detail_render_by_stage -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing run, not a check"]
    fn time_a_full_size_detail_render_by_stage() {
        use std::time::{Duration, Instant};

        use gtp_core::{Frame, FrameFlag, FrameId, FrameKind, SourceFile, SourceRole};
        use gtp_io::{
            from_f32_linear, read_tiff16, to_f32_linear, write_tiff16, Compression, Limits,
        };

        use crate::library_fs::originals_rel;
        use crate::services::export::read_full;
        use crate::services::testkit::Fixture;

        const RUNS: usize = 5;
        let (width, height) = (6000u32, 4000u32);

        // A scan with colour (so the mixer's gate opens) and grain, and an infrared plane with specks.
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                let (fx, fy) = (x as f32 / width as f32, y as f32 / height as f32);
                let px = y * width + x;
                let n = |k: u32| {
                    ((px.wrapping_add(k).wrapping_mul(2_654_435_761) >> 16) & 0xff) as f32 / 3000.0
                };
                data.extend_from_slice(&[
                    0.08 + 0.55 * fx + n(0),
                    0.1 + 0.5 * fy * (1.0 - 0.5 * fx) + n(1),
                    0.06 + 0.45 * (1.0 - fx) * (0.4 + 0.6 * fy) + n(2),
                ]);
            }
        }
        let scan = ImageF32::new(width, height, 3, data).unwrap();
        let mut plane: Vec<f32> = (0..width * height)
            .map(|i| 0.9 + (i.wrapping_mul(2_246_822_519) >> 28) as f32 / 1000.0)
            .collect();
        for k in 0..300u32 {
            let cx = k.wrapping_mul(2_654_435_761) % (width - 8);
            let cy = k.wrapping_mul(40_503).wrapping_add(7919) % (height - 8);
            for dy in 0..5 {
                for dx in 0..5 {
                    plane[((cy + dy) * width + cx + dx) as usize] = 0.3;
                }
            }
        }
        let ir = ImageF32::new(width, height, 1, plane).unwrap();

        // Two frames of one library: originals compressed as the import writes them (deflate with
        // the predictor), and uncompressed.
        let fx = Fixture::new("p31-detail");
        let frame_of = |index: u32, compression: Compression| -> (Frame, Vec<SourceFile>) {
            let id = FrameId::new();
            let rel = originals_rel(fx.roll, id).join("scan.tif");
            let ir_rel = originals_rel(fx.roll, id).join("ir.tif");
            let path = fx.library.resolve(&rel).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            write_tiff16(&path, &from_f32_linear(&scan), compression).unwrap();
            let ir_path = fx.library.resolve(&ir_rel).unwrap();
            write_tiff16(&ir_path, &from_f32_linear(&ir), compression).unwrap();
            let frame = Frame {
                id,
                roll_id: fx.roll,
                index_in_roll: index,
                kind: FrameKind::ScanRgbIr,
                width,
                height,
                has_ir: true,
                note: String::new(),
                flag: FrameFlag::None,
                created_at: String::new(),
                edited_at: None,
            };
            let source = |role, rel: &std::path::Path| SourceFile {
                frame_id: id,
                role,
                rel_path: rel.to_string_lossy().replace('\\', "/"),
                sha256: String::new(),
                bytes: 0,
            };
            let sources = vec![
                source(SourceRole::Rgb, &rel),
                source(SourceRole::Ir, &ir_rel),
            ];
            fx.library
                .write(|c| {
                    c.insert_frame(&frame)?;
                    for s in &sources {
                        c.insert_source(s)?;
                    }
                    Ok(())
                })
                .unwrap();
            (frame, sources)
        };
        let (frame, sources) = frame_of(1, Compression::DeflatePredictor);
        let (_, sources_raw) = frame_of(2, Compression::None);
        let path_of = |s: &SourceFile| {
            fx.library
                .resolve(std::path::Path::new(&s.rel_path))
                .unwrap()
        };
        let size_of = |sources: &[SourceFile]| {
            sources
                .iter()
                .map(|s| std::fs::metadata(path_of(s)).unwrap().len())
                .collect::<Vec<_>>()
        };
        eprintln!(
            "files (scan, ir) in bytes: deflate {:?}, uncompressed {:?}",
            size_of(&sources),
            size_of(&sources_raw)
        );

        // Every tool on.
        let mut recipe = Recipe::default();
        recipe.noise.luminance = 50.0;
        recipe.noise.color = 50.0;
        recipe.color.red_cyan = 10.0;
        recipe.color.green_magenta = -8.0;
        recipe.color.blue_yellow = 12.0;
        recipe.color.temperature = 15.0;
        recipe.color.tint = -10.0;
        recipe.color.saturation = 1.2;
        recipe.color.vibrance = 25.0;
        for band in 0..8 {
            recipe.color.mixer.hue[band] = 5.0 + band as f32;
            recipe.color.mixer.saturation[band] = 10.0 - band as f32;
            recipe.color.mixer.lightness[band] = -4.0 + band as f32;
        }
        recipe.tone.exposure_ev = 0.3;
        recipe.tone.contrast = 20.0;
        recipe.tone.highlights = -15.0;
        recipe.tone.shadows = 20.0;
        recipe.tone.whites = 5.0;
        recipe.tone.blacks = -5.0;
        recipe.tone.dehaze = 10.0;
        recipe.tone.curve.master = vec![[0.0, 0.0], [0.25, 0.2], [0.75, 0.8], [1.0, 1.0]];
        recipe.tone.curve.r = vec![[0.0, 0.0], [0.5, 0.55], [1.0, 1.0]];
        recipe.color.split_tone.shadow_hue = 220.0;
        recipe.color.split_tone.shadow_strength = 20.0;
        recipe.color.split_tone.highlight_hue = 40.0;
        recipe.color.split_tone.highlight_strength = 15.0;
        recipe.geometry.angle_deg = 0.5;
        recipe.tone.vignette.amount = -30.0;
        recipe.output.sharpen.amount = 60.0;
        let recipe = recipe.clamped();

        let median = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        // Rows of (name, one value per counted run), in order of first appearance.
        fn put(rows: &mut Vec<(String, Vec<f64>)>, name: &str, value: f64) {
            match rows.iter_mut().find(|(n, _)| n == name) {
                Some((_, v)) => v.push(value),
                None => rows.push((name.to_string(), vec![value])),
            }
        }

        for threads in [6usize, 12] {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            let mut rows: Vec<(String, Vec<f64>)> = Vec::new();
            pool.install(|| {
                for run in 0..=RUNS {
                    let mut out = Vec::new();
                    let wall = Instant::now();
                    let t = Instant::now();
                    let (full, ir) = read_full(&fx.library, &frame, &sources, true).unwrap();
                    put(&mut out, "decode (read_full: scan + ir)", ms(t.elapsed()));
                    let input = PipelineInput {
                        rgb: Arc::new(full),
                        ir: ir.map(Arc::new),
                        film_base: None,
                        full_long_edge: None,
                    };
                    let started = Instant::now();
                    let mut marks = Vec::new();
                    let mut image =
                        render_traced(&input, &recipe, RenderTarget::Full, &mut |stage| {
                            marks.push((stage, Instant::now()));
                        })
                        .unwrap();
                    let ended = Instant::now();
                    put(
                        &mut out,
                        "copy of the scan (start of render)",
                        ms(marks[0].1 - started),
                    );
                    for (n, (stage, at)) in marks.iter().enumerate() {
                        let until = marks.get(n + 1).map_or(ended, |next| next.1);
                        put(&mut out, &format!("stage {stage:?}"), ms(until - *at));
                    }
                    let t = Instant::now();
                    drop(input);
                    put(&mut out, "drop of the input", ms(t.elapsed()));
                    let t = Instant::now();
                    gtp_edit::apply_sharpen(&mut image, &recipe.output.sharpen).unwrap();
                    put(&mut out, "output sharpening", ms(t.elapsed()));
                    let t = Instant::now();
                    let rgba = to_rgba8_srgb(&image).unwrap();
                    put(&mut out, "to_rgba8_srgb", ms(t.elapsed()));
                    let t = Instant::now();
                    drop(image);
                    put(&mut out, "drop of the image", ms(t.elapsed()));
                    put(
                        &mut out,
                        "TOTAL (wall, decode to RGBA8)",
                        ms(wall.elapsed()),
                    );
                    // The straighten angle crops the picture a little.
                    assert!(rgba.len() > (width * height * 4) as usize / 10 * 9);
                    if run > 0 {
                        for (name, v) in out {
                            put(&mut rows, &name, v[0]);
                        }
                    }
                }
            });
            eprintln!("--- {threads} threads, median of {RUNS} runs, ms ---");
            for (name, values) in rows {
                eprintln!("{name:<40} {:>9.1}", median(values));
            }

            // The decode of the 3-channel scan, taken apart.
            let mut parts: Vec<(String, Vec<f64>)> = Vec::new();
            pool.install(|| {
                for (label, sources) in [("deflate", &sources), ("uncompressed", &sources_raw)] {
                    let path = path_of(&sources[0]);
                    for run in 0..=RUNS {
                        let t = Instant::now();
                        let bytes = std::fs::read(&path).unwrap();
                        let read = ms(t.elapsed());
                        drop(bytes);
                        let t = Instant::now();
                        let tiff = read_tiff16(&path, &Limits::default()).unwrap();
                        let decoded = ms(t.elapsed());
                        let t = Instant::now();
                        let converted_image = to_f32_linear(&tiff).unwrap();
                        let converted = ms(t.elapsed());
                        drop((tiff, converted_image));
                        if run > 0 {
                            put(
                                &mut parts,
                                &format!("{label}: std::fs::read of the file"),
                                read,
                            );
                            put(
                                &mut parts,
                                &format!("{label}: read_tiff16 (open, read, decode)"),
                                decoded,
                            );
                            put(&mut parts, &format!("{label}: to_f32_linear"), converted);
                        }
                    }
                }
            });
            eprintln!("--- decode of the 3-channel scan alone, {threads} threads, ms ---");
            for (name, values) in parts {
                eprintln!("{name:<50} {:>9.1}", median(values));
            }
        }

        // Does the dust stage do anything on this fixture?
        let mut probe = scan.clone();
        let report = gtp_edit::remove_dust(&mut probe, &ir, &recipe.dust).unwrap();
        eprintln!(
            "dust on the fixture: {} pixels masked, applied: {}",
            report.masked, report.applied
        );
    }
}
