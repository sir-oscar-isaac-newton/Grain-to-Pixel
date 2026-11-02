//! Fitting a Frontier-style look to reference renders (P-20, decisions/0009-frontier-look.md).
//!
//! The look is four numbers ([`FrontierParams`]): the slope and pivot of a sigmoid tone curve, a
//! gamma after it, and the saturation. Every candidate is rendered through the real pipeline (the
//! curve and the saturation put into the recipe) on a small copy of each frame, laid over the part
//! of the picture the reference covers (found by registration, as in the report), and judged by the
//! mean CIEDE2000, each pixel capped so a few outliers do not rule. The search is Nelder-Mead from
//! a neutral start. Some of the frames are held out of the fit and judged afterwards, which is the
//! number that says whether the look generalizes.

use std::sync::Arc;

use gtp_app::{
    analyze_with_levels, build_proxy, frontier_curve, render, FrameHint, FrontierParams,
    LevelsPercentiles, PipelineInput, RenderTarget,
};
use gtp_core::recipe::{Crop, Invert};
use gtp_core::{AppError, ErrorCode, FilmFormat, ImageF32, Look, Recipe};

use crate::manifest::Manifest;
use crate::register::{crop_resample, luma, register_oriented, Window};
use crate::report::{
    compare, load_frame, load_reference_at, mean_chroma_difference, percentile, roll_base,
    srgb_floats, MIN_SCORE,
};

/// Columns of the references the fit compares, and the longest edge of our copy of each frame.
const FIT_COMPARE_WIDTH: usize = 256;
const FIT_PROXY_EDGE: u32 = 768;
/// A pixel counts for at most this ΔE in the objective.
pub const DE_CAP: f32 = 40.0;
/// The most evaluations of the search.
const MAX_EVALUATIONS: usize = 220;

/// The frames, split into the ones fitted and the ones held out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    pub train: Vec<usize>,
    pub test: Vec<usize>,
}

/// Holds out every `every`-th frame (the 3rd, 6th, 9th, ... for 3); the rest are fitted. With
/// `every` below 2, or a count under 2, nothing is held out.
pub fn split(count: usize, every: usize) -> Split {
    if every < 2 || count < 2 {
        return Split {
            train: (0..count).collect(),
            test: Vec::new(),
        };
    }
    let (test, train): (Vec<usize>, Vec<usize>) = (0..count).partition(|i| (i + 1) % every == 0);
    Split { train, test }
}

/// Nelder-Mead minimization of `f` from `start`, with initial steps `steps`, for at most
/// `max_evaluations` evaluations. Returns the best point and its value. The function is expected to
/// bring a point inside its own limits.
pub fn nelder_mead(
    mut f: impl FnMut(&[f64; 4]) -> f64,
    start: [f64; 4],
    steps: [f64; 4],
    max_evaluations: usize,
) -> ([f64; 4], f64) {
    let mut evaluations = 0usize;
    let mut eval = |p: &[f64; 4], evaluations: &mut usize| {
        *evaluations += 1;
        f(p)
    };
    let mut simplex: Vec<([f64; 4], f64)> = Vec::with_capacity(5);
    simplex.push((start, eval(&start, &mut evaluations)));
    for i in 0..4 {
        let mut p = start;
        p[i] += steps[i];
        simplex.push((p, eval(&p, &mut evaluations)));
    }
    while evaluations < max_evaluations {
        simplex.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (best, worst) = (simplex[0].1, simplex[4].1);
        if (worst - best).abs() < 1e-5 {
            break;
        }
        let mut centroid = [0.0; 4];
        for (p, _) in &simplex[..4] {
            for i in 0..4 {
                centroid[i] += p[i] / 4.0;
            }
        }
        let along = |t: f64, from: &[f64; 4]| {
            let mut p = [0.0; 4];
            for i in 0..4 {
                p[i] = centroid[i] + t * (centroid[i] - from[i]);
            }
            p
        };
        let worst_point = simplex[4].0;
        let reflected = along(1.0, &worst_point);
        let fr = eval(&reflected, &mut evaluations);
        if fr < simplex[0].1 {
            let expanded = along(2.0, &worst_point);
            let fe = eval(&expanded, &mut evaluations);
            simplex[4] = if fe < fr {
                (expanded, fe)
            } else {
                (reflected, fr)
            };
        } else if fr < simplex[3].1 {
            simplex[4] = (reflected, fr);
        } else {
            let contracted = along(-0.5, &worst_point);
            let fc = eval(&contracted, &mut evaluations);
            if fc < simplex[4].1 {
                simplex[4] = (contracted, fc);
            } else {
                // Shrink towards the best.
                let best_point = simplex[0].0;
                for entry in simplex.iter_mut().skip(1) {
                    for (value, best) in entry.0.iter_mut().zip(best_point) {
                        *value = best + 0.5 * (*value - best);
                    }
                    entry.1 = eval(&entry.0, &mut evaluations);
                }
            }
        }
    }
    simplex.sort_by(|a, b| a.1.total_cmp(&b.1));
    simplex[0]
}

/// One frame prepared for the fit.
struct FrameData {
    name: String,
    proxy: Arc<ImageF32>,
    base: [f32; 3],
    /// Where the reference sits in our render of the whole capture.
    window: Window,
    reference: ImageF32,
    /// The automatic levels of the frame for each percentile setting.
    levels: Vec<Invert>,
}

impl FrameData {
    /// The frame rendered with `look` (none is the neutral conversion) and compared with the
    /// reference: the ΔE of every pixel, and our mean chroma minus the reference's.
    fn deltas(
        &self,
        setting: usize,
        look: Option<&FrontierParams>,
    ) -> Result<(Vec<f32>, f32), AppError> {
        let mut recipe = Recipe::default();
        recipe.geometry.crop = Crop::default();
        recipe.base.rgb = self.base;
        recipe.invert = self.levels[setting];
        if let Some(params) = look {
            recipe.tone.curve.master = frontier_curve(params);
            recipe.color.saturation = params.saturation as f32;
        }
        let input = PipelineInput {
            rgb: Arc::clone(&self.proxy),
            ir: None,
            film_base: Some(self.base),
        };
        let ours = srgb_floats(&render(&input, &recipe.clamped(), RenderTarget::Full)?)?;
        let ours = crop_resample(
            &ours,
            self.window,
            self.reference.width as usize,
            self.reference.height as usize,
        )?;
        let chroma = mean_chroma_difference(&ours, &self.reference);
        Ok((compare(&ours, &self.reference)?.0, chroma))
    }
}

/// How a set of frames fared: the median of the frames' median ΔE, the mean ΔE (capped), and our
/// mean chroma minus the reference's (positive when ours is the more saturated).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    pub median_of_medians: f32,
    pub mean: f32,
    pub chroma: f32,
}

/// What one percentile setting gave.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingResult {
    pub percentiles: LevelsPercentiles,
    pub params: FrontierParams,
    /// The neutral conversion, and the fitted look, on the fitted frames and on the held-out ones.
    pub neutral_train: Score,
    pub neutral_test: Option<Score>,
    pub fitted_train: Score,
    pub fitted_test: Option<Score>,
}

/// The whole fit.
#[derive(Debug, Clone, PartialEq)]
pub struct FitReport {
    pub train: Vec<String>,
    pub test: Vec<String>,
    /// Pairs that did not register well enough and were left out.
    pub excluded: Vec<String>,
    pub settings: Vec<SettingResult>,
}

fn score(
    frames: &[&FrameData],
    setting: usize,
    look: Option<&FrontierParams>,
) -> Result<Score, AppError> {
    let mut medians = Vec::new();
    let (mut sum, mut count, mut chroma) = (0.0f64, 0usize, 0.0f64);
    for frame in frames {
        let (des, frame_chroma) = frame.deltas(setting, look)?;
        chroma += f64::from(frame_chroma);
        sum += des.iter().map(|d| f64::from(d.min(DE_CAP))).sum::<f64>();
        count += des.len();
        let mut by_value = des;
        by_value.sort_unstable_by(f32::total_cmp);
        medians.push(percentile(&by_value, 0.5));
    }
    medians.sort_unstable_by(f32::total_cmp);
    Ok(Score {
        median_of_medians: percentile(&medians, 0.5),
        mean: (sum / count.max(1) as f64) as f32,
        chroma: (chroma / frames.len().max(1) as f64) as f32,
    })
}

/// Fits the look to the manifest's pairs for every percentile setting in `settings`, holding out
/// every `holdout_every`-th registered frame. `note` hears of each step.
pub fn run(
    manifest: &Manifest,
    holdout_every: usize,
    settings: &[LevelsPercentiles],
    note: &mut dyn FnMut(&str),
) -> Result<FitReport, AppError> {
    run_with_budget(manifest, holdout_every, settings, MAX_EVALUATIONS, note)
}

/// [`run`] with at most `evaluations` renderings of the candidate look for each setting.
pub fn run_with_budget(
    manifest: &Manifest,
    holdout_every: usize,
    settings: &[LevelsPercentiles],
    evaluations: usize,
    note: &mut dyn FnMut(&str),
) -> Result<FitReport, AppError> {
    if settings.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Give at least one percentile setting to fit with.",
        ));
    }
    let hint = (manifest.format != FilmFormat::Mm35).then(|| FrameHint {
        aspect: manifest.format.aspect(),
        size: None,
    });

    // One film base per roll, as the report measures it.
    let mut rolls: Vec<&str> = Vec::new();
    for pair in &manifest.pairs {
        if !rolls.contains(&pair.roll.as_str()) {
            rolls.push(&pair.roll);
        }
    }
    let mut bases = Vec::new();
    for roll in &rolls {
        let base = match manifest.film_base {
            Some(base) => base,
            None => {
                let mut estimates = Vec::new();
                for pair in manifest.pairs.iter().filter(|p| p.roll == *roll) {
                    note(&format!("measuring the film base on {}", pair.name));
                    let input = PipelineInput {
                        rgb: Arc::new(load_frame(pair)?),
                        ir: None,
                        film_base: None,
                    };
                    estimates.push(
                        analyze_with_levels(
                            &input,
                            hint,
                            Look::Neutral,
                            LevelsPercentiles::DEFAULT,
                        )?
                        .base,
                    );
                }
                roll_base(&estimates)
                    .ok_or_else(|| AppError::internal("no film base was measured"))?
                    .rgb
            }
        };
        bases.push(base);
    }

    // Each frame: its small copy, where the reference sits in it, and its levels per setting.
    let mut frames: Vec<FrameData> = Vec::new();
    let mut excluded = Vec::new();
    for pair in &manifest.pairs {
        note(&format!("preparing {}", pair.name));
        let base = bases[rolls.iter().position(|r| *r == pair.roll).unwrap_or(0)];
        let image = load_frame(pair)?;
        let input = PipelineInput {
            rgb: Arc::new(image),
            ir: None,
            film_base: Some(base),
        };
        let proxy = Arc::new(build_proxy(&input.rgb, FIT_PROXY_EDGE)?);
        let mut levels = Vec::new();
        for setting in settings {
            levels.push(
                analyze_with_levels(&input, hint, Look::Neutral, *setting)?
                    .auto_recipe
                    .invert,
            );
        }
        let stored = load_reference_at(pair, FIT_COMPARE_WIDTH)?;
        let mut neutral = Recipe::default();
        neutral.geometry.crop = Crop::default();
        neutral.base.rgb = base;
        neutral.invert = levels[0];
        let proxy_input = PipelineInput {
            rgb: Arc::clone(&proxy),
            ir: None,
            film_base: Some(base),
        };
        let full = srgb_floats(&render(&proxy_input, &neutral, RenderTarget::Full)?)?;
        let found = register_oriented(&luma(&full), &luma(&stored))
            .filter(|(registration, _)| registration.score >= MIN_SCORE);
        match found {
            Some((registration, orientation)) => frames.push(FrameData {
                name: pair.name.clone(),
                proxy,
                base,
                window: registration.window,
                reference: orientation.apply_image(&stored)?,
                levels,
            }),
            None => excluded.push(pair.name.clone()),
        }
    }
    if frames.len() < 2 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Fewer than two pairs registered, so there is nothing to fit.",
        )
        .with_details(format!("{} registered", frames.len())));
    }

    let Split { train, test } = split(frames.len(), holdout_every);
    let pick =
        |indices: &[usize]| -> Vec<&FrameData> { indices.iter().map(|i| &frames[*i]).collect() };
    let (train_frames, test_frames) = (pick(&train), pick(&test));

    let mut results = Vec::new();
    for (setting_index, percentiles) in settings.iter().enumerate() {
        note(&format!(
            "fitting with levels at {} and {}",
            percentiles.low, percentiles.high
        ));
        let neutral_train = score(&train_frames, setting_index, None)?;
        let neutral_test = (!test_frames.is_empty())
            .then(|| score(&test_frames, setting_index, None))
            .transpose()?;

        let objective = |p: &[f64; 4]| -> f64 {
            let params = FrontierParams::from_array(*p);
            match score(&train_frames, setting_index, Some(&params)) {
                // The mean ΔE shapes the tone; the chroma term keeps the saturation honest. Without
                // it the search turns the saturation down to hide colour casts it cannot fix.
                Ok(s) => f64::from(s.mean) + f64::from(s.chroma.abs()),
                Err(_) => f64::from(DE_CAP),
            }
        };
        let start = FrontierParams {
            slope: 4.0,
            pivot: 0.5,
            gamma: 1.0,
            saturation: 1.0,
        };
        let (best, _) = nelder_mead(
            objective,
            start.to_array(),
            [2.0, 0.1, 0.25, 0.15],
            evaluations,
        );
        let params = FrontierParams::from_array(best);
        results.push(SettingResult {
            percentiles: *percentiles,
            params,
            neutral_train,
            neutral_test,
            fitted_train: score(&train_frames, setting_index, Some(&params))?,
            fitted_test: (!test_frames.is_empty())
                .then(|| score(&test_frames, setting_index, Some(&params)))
                .transpose()?,
        });
    }
    Ok(FitReport {
        train: train_frames.iter().map(|f| f.name.clone()).collect(),
        test: test_frames.iter().map(|f| f.name.clone()).collect(),
        excluded,
        settings: results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_nth_frame_is_held_out() {
        assert_eq!(
            split(15, 3),
            Split {
                train: vec![0, 1, 3, 4, 6, 7, 9, 10, 12, 13],
                test: vec![2, 5, 8, 11, 14],
            }
        );
        // Nothing is held out when asked not to, or with too few frames to spare.
        assert_eq!(split(5, 0).test, Vec::<usize>::new());
        assert_eq!(split(5, 1).test, Vec::<usize>::new());
        assert_eq!(split(1, 3).train, vec![0]);
        assert_eq!(
            split(0, 3),
            Split {
                train: vec![],
                test: vec![]
            }
        );
    }

    #[test]
    fn the_search_finds_the_minimum_of_a_smooth_function_and_stops() {
        let target = [3.0, 0.4, 1.5, 1.2];
        let mut calls = 0;
        let (best, value) = nelder_mead(
            |p| {
                calls += 1;
                p.iter().zip(target).map(|(a, b)| (a - b).powi(2)).sum()
            },
            [1.0, 0.5, 1.0, 1.0],
            [1.0, 0.2, 0.5, 0.3],
            500,
        );
        assert!(value < 1e-3, "value {value}");
        for (got, want) in best.iter().zip(target) {
            assert!((got - want).abs() < 0.05, "{best:?}");
        }
        assert!(calls <= 500);
        // It never uses more evaluations than it is given.
        let mut few = 0;
        nelder_mead(
            |p| {
                few += 1;
                p[0] * p[0]
            },
            [5.0, 0.0, 0.0, 0.0],
            [1.0; 4],
            12,
        );
        assert!(few <= 12 + 5, "{few}");
    }

    /// Three synthetic scans, and references made of our own render of each with a known Frontier
    /// look, so the fit has an answer to find.
    fn known_look_manifest(label: &str, known: &FrontierParams) -> (std::path::PathBuf, Manifest) {
        use gtp_app::analyze_with_levels;
        use gtp_core::testing::synthetic::{self, Params, Pattern};
        use gtp_io::{from_f32_linear, to_output, write_tiff16, Compression, Tiff16};

        let dir = std::env::temp_dir().join(format!("gtp-fit-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut pairs = Vec::new();
        for (i, pattern) in [Pattern::Blobs, Pattern::Gradient, Pattern::Blobs]
            .into_iter()
            .enumerate()
        {
            let s = synthetic::negative(
                20 + i as u64,
                &Params {
                    width: 160,
                    height: 110,
                    rebate: [8; 4],
                    pattern,
                    noise: 0.01,
                    ..Params::default()
                },
            )
            .unwrap();
            write_tiff16(
                &dir.join(format!("scan{i}.tif")),
                &from_f32_linear(&s.image),
                Compression::None,
            )
            .unwrap();
            // The reference: the whole capture with the known look, through the real pipeline.
            let input = PipelineInput {
                rgb: Arc::new(s.image.clone()),
                ir: None,
                film_base: Some(s.truth.base),
            };
            let levels =
                analyze_with_levels(&input, None, Look::Neutral, LevelsPercentiles::DEFAULT)
                    .unwrap()
                    .auto_recipe
                    .invert;
            let mut recipe = Recipe::default();
            recipe.geometry.crop = Crop::default();
            recipe.base.rgb = s.truth.base;
            recipe.invert = levels;
            recipe.tone.curve.master = frontier_curve(known);
            recipe.color.saturation = known.saturation as f32;
            let shown = to_output(
                &render(&input, &recipe.clamped(), RenderTarget::Full).unwrap(),
                gtp_core::recipe::OutputSpace::Srgb,
            )
            .unwrap();
            write_tiff16(
                &dir.join(format!("ref{i}.tif")),
                &Tiff16 {
                    width: shown.width,
                    height: shown.height,
                    channels: 3,
                    data: shown.data,
                },
                Compression::None,
            )
            .unwrap();
            pairs.push(format!(
                r#"{{"name":"f{i}","roll":"r","scan":"scan{i}.tif","reference":"ref{i}.tif"}}"#
            ));
        }
        let manifest = crate::manifest::parse(
            &format!(r#"{{"format":"35mm","pairs":[{}]}}"#, pairs.join(",")),
            &dir,
        )
        .unwrap();
        (dir, manifest)
    }

    #[test]
    fn the_fit_finds_a_known_look_and_the_held_out_frame_agrees() {
        let known = FrontierParams {
            slope: 7.0,
            pivot: 0.45,
            gamma: 1.3,
            saturation: 1.25,
        };
        let (dir, mut manifest) = known_look_manifest("known", &known);
        // The rolls' base is measured on the frames, as the report does.
        manifest.film_base = None;
        let report =
            run_with_budget(&manifest, 3, &[LevelsPercentiles::DEFAULT], 90, &mut |_| {}).unwrap();
        assert_eq!(report.train, ["f0", "f1"]);
        assert_eq!(report.test, ["f2"]);
        assert!(report.excluded.is_empty(), "{:?}", report.excluded);
        let r = &report.settings[0];
        // The neutral conversion is far from the reference, the fitted look close to it, and the
        // frame the fit never saw is as close as the ones it did.
        assert!(r.neutral_train.mean > 4.0, "{:?}", r.neutral_train);
        assert!(r.fitted_train.mean < 1.5, "{:?}", r.fitted_train);
        let held_out = r.fitted_test.expect("one frame is held out");
        assert!(held_out.mean < 2.0, "{held_out:?}");
        assert!(held_out.mean < r.neutral_test.unwrap().mean / 2.0);
        // The saturation is held to the reference's, not turned down to hide an error.
        assert!(r.fitted_train.chroma.abs() < 2.0, "{:?}", r.fitted_train);
        // The numbers found stay inside their ranges, and the saturation is near the known one.
        assert!(
            (r.params.saturation - known.saturation).abs() < 0.3,
            "{:?}",
            r.params
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fit_with_nothing_to_fit_is_refused() {
        let manifest = Manifest {
            format: FilmFormat::Mm35,
            film_base: None,
            pairs: Vec::new(),
        };
        let err = run(&manifest, 3, &[], &mut |_| {}).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }
}
