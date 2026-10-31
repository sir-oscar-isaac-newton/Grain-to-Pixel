//! The render pipeline: one negative and one Recipe in, one linear Rec.2020 positive out.
//!
//! Stages run in the fixed order of [`STAGE_ORDER`]. The crop is applied early as an exact copy, so
//! later stages work on the frame only, and auto levels never see the rebate; straighten, turns
//! and flips come last, so there is still at most one resample. `render` never runs anything
//! automatic: it uses the recipe as given, and [`analyze`] is what fills in the automatic values.

mod analyze;
mod display;
mod proxy;
mod stages;

use std::sync::Arc;

use gtp_core::{AppError, ErrorCode, ImageF32, Recipe};

pub use analyze::{analyze, Analysis};
pub use display::{histogram, to_rgba8_srgb, Histogram};
pub use proxy::{build_proxy, PREVIEW_EDGE, PROXY_EDGE};
pub use stages::{Stage, STAGE_ORDER};
/// What a render starts from.
#[derive(Debug, Clone)]
pub struct PipelineInput {
    /// The scan (or a proxy of it): linear RGB, 3 channels.
    pub rgb: Arc<ImageF32>,
    /// The infrared plane, if the scan has one (used by dust removal, P-12).
    pub ir: Option<Arc<ImageF32>>,
    /// The roll's measured film base, if there is one.
    pub film_base: Option<[f32; 3]>,
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

/// [`render`], reporting each stage as it starts (used to test the order).
pub(crate) fn render_traced(
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
    for stage in STAGE_ORDER {
        trace(stage);
        image = stages::run_stage(stage, image, input, &recipe)?;
    }
    Ok(image)
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
        image = stages::run_stage(stage, image, input, &recipe)?;
    }
    Ok(image)
}

/// The frame as the Color stage receives it: [`normalized_frame`] followed by the Invert stage of
/// `recipe`. This is what the neutral picker measures.
pub(crate) fn inverted_frame(input: &PipelineInput, recipe: &Recipe) -> Result<ImageF32, AppError> {
    let recipe = recipe.clone().clamped();
    let image = normalized_frame(input, &recipe)?;
    stages::run_stage(Stage::Invert, image, input, &recipe)
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
                Stage::Color,
                Stage::Tone,
                Stage::Geometry
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

    #[test]
    fn bad_input_is_an_error() {
        let gray = PipelineInput {
            rgb: Arc::new(ImageF32::zeros(4, 4, 1).unwrap()),
            ir: None,
            film_base: None,
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
}
