//! Golden-image regression tests: fixed synthetic negatives rendered with fixed recipes, compared
//! with the renders stored in `samples/public/golden/` by their per-pixel CIEDE2000.
//!
//! A golden file is the render as a 16-bit RGB TIFF holding `value / GOLDEN_SCALE`, so linear
//! values up to `GOLDEN_SCALE` survive (higher ones are clipped). A fresh render goes through the
//! same 16-bit rounding before it is compared, so an unchanged pipeline differs by exactly 0 on the
//! machine that blessed it; [`MAX_DELTA_E`] absorbs the last-digit float differences between
//! operating systems.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gtp_app::{analyze, render, PipelineInput, RenderTarget};
use gtp_core::recipe::{BaseSource, Crop, CurvePoint};
use gtp_core::testing::synthetic::{self, Params, Pattern, Synthetic};
use gtp_core::{AppError, ImageF32, Recipe};
use gtp_io::{from_f32_linear, read_tiff16, to_f32_linear, write_tiff16, Compression, Limits};

use crate::diff::{delta_e_map, ImageDiff};

/// Linear value stored as 65535 in a golden file.
pub const GOLDEN_SCALE: f32 = 4.0;
/// A case fails when any pixel differs from its golden by more than this ΔE2000 (well below
/// anything visible).
pub const MAX_DELTA_E: f32 = 0.5;

const WIDTH: u32 = 240;
const HEIGHT: u32 = 160;
const REBATE: u32 = 12;

/// One golden case: a named negative-and-recipe pair.
#[derive(Debug, Clone, Copy)]
pub struct Case {
    pub name: &'static str,
    /// What the case exercises.
    pub about: &'static str,
    build: fn() -> Result<ImageF32, AppError>,
}

impl Case {
    /// Renders the case as it would be stored (16-bit rounded).
    pub fn render(&self) -> Result<ImageF32, AppError> {
        stored_round_trip(&(self.build)()?)
    }

    /// The golden file of this case in `dir`.
    pub fn path_in(&self, dir: &Path) -> PathBuf {
        dir.join(format!("{}.tif", self.name))
    }
}

/// Every golden case, in a fixed order.
pub const CASES: [Case; 6] = [
    Case {
        name: "base-levels",
        about: "crop, manual film base and levels, nothing else",
        build: base_levels,
    },
    Case {
        name: "colour",
        about: "colour axes, temperature, tint, saturation and vibrance",
        build: colour,
    },
    Case {
        name: "tone",
        about: "exposure, contrast, highlights, shadows, whites, blacks, dehaze and curves",
        build: tone,
    },
    Case {
        name: "mono-red-filter",
        about: "black and white with a red-filter mix",
        build: mono_red_filter,
    },
    Case {
        name: "geometry",
        about: "straighten, a quarter turn and a flip",
        build: geometry,
    },
    Case {
        name: "auto",
        about: "the automatic path: analyze, then render its recipe",
        build: auto,
    },
];

/// The folder of the committed goldens.
pub fn default_dir() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    // tools/benchmark -> the repository root.
    let root = manifest.ancestors().nth(2).unwrap_or(manifest);
    root.join("samples").join("public").join("golden")
}

/// How one case compares with its golden.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub name: &'static str,
    /// The difference from the stored golden; `None` when there is none yet.
    pub diff: Option<ImageDiff>,
}

impl Outcome {
    pub fn passed(&self) -> bool {
        self.diff.is_some_and(|d| d.max <= MAX_DELTA_E)
    }
}

/// Compares every case with its golden in `dir`. When `maps` is given, the ΔE map of each case
/// that differs is written there as a 16-bit gray TIFF (ΔE 10 or more is white).
pub fn check(dir: &Path, maps: Option<&Path>) -> Result<Vec<Outcome>, AppError> {
    CASES
        .iter()
        .map(|case| {
            let current = case.render()?;
            let diff = compare_with_stored(case, dir, &current, maps)?;
            Ok(Outcome {
                name: case.name,
                diff,
            })
        })
        .collect()
}

/// Rewrites every golden in `dir` from the current pipeline and reports how far each new render
/// is from the one it replaces (`None` for a new case), for the pull request.
pub fn bless(dir: &Path) -> Result<Vec<Outcome>, AppError> {
    std::fs::create_dir_all(dir).map_err(|err| AppError::from_io(&err))?;
    CASES
        .iter()
        .map(|case| {
            let current = case.render()?;
            let diff = compare_with_stored(case, dir, &current, None)?;
            write_tiff16(
                &case.path_in(dir),
                &from_f32_linear(&scaled(&current, 1.0 / GOLDEN_SCALE)),
                Compression::DeflatePredictor,
            )?;
            Ok(Outcome {
                name: case.name,
                diff,
            })
        })
        .collect()
}

fn compare_with_stored(
    case: &Case,
    dir: &Path,
    current: &ImageF32,
    maps: Option<&Path>,
) -> Result<Option<ImageDiff>, AppError> {
    let path = case.path_in(dir);
    if !path.exists() {
        return Ok(None);
    }
    let stored = read_stored(&path)?;
    if (stored.width, stored.height) != (current.width, current.height) {
        // A new size is a change of the whole image.
        return Ok(Some(ImageDiff {
            mean: f32::INFINITY,
            p95: f32::INFINITY,
            max: f32::INFINITY,
        }));
    }
    let map = delta_e_map(&stored, current)?;
    let diff = ImageDiff::of_map(&map);
    if let Some(maps) = maps.filter(|_| diff.max > 0.0) {
        std::fs::create_dir_all(maps).map_err(|err| AppError::from_io(&err))?;
        write_tiff16(
            &maps.join(format!("{}-delta-e.tif", case.name)),
            &from_f32_linear(&scaled(&map, 0.1)),
            Compression::DeflatePredictor,
        )?;
    }
    Ok(Some(diff))
}

fn read_stored(path: &Path) -> Result<ImageF32, AppError> {
    let stored = to_f32_linear(&read_tiff16(path, &Limits::default())?)?;
    Ok(scaled(&stored, GOLDEN_SCALE))
}

fn scaled(image: &ImageF32, factor: f32) -> ImageF32 {
    let mut out = image.clone();
    out.data.iter_mut().for_each(|v| *v *= factor);
    out
}

/// The image after a trip through the stored form, without touching the disk.
fn stored_round_trip(image: &ImageF32) -> Result<ImageF32, AppError> {
    let stored = from_f32_linear(&scaled(image, 1.0 / GOLDEN_SCALE));
    Ok(scaled(&to_f32_linear(&stored)?, GOLDEN_SCALE))
}

fn negative(seed: u64, pattern: Pattern) -> Result<Synthetic, AppError> {
    synthetic::negative(
        seed,
        &Params {
            width: WIDTH,
            height: HEIGHT,
            rebate: [REBATE; 4],
            pattern,
            ..Params::default()
        },
    )
}

/// The recipe that recovers the true positive of `s`: its frame, base and levels.
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
    recipe.invert.black = s.truth.black;
    recipe.invert.white = s.truth.white;
    recipe
}

fn render_full(s: &Synthetic, recipe: &Recipe) -> Result<ImageF32, AppError> {
    let input = PipelineInput {
        rgb: Arc::new(s.image.clone()),
        ir: None,
        film_base: None,
    };
    render(&input, recipe, RenderTarget::Full)
}

fn base_levels() -> Result<ImageF32, AppError> {
    let s = negative(1, Pattern::Gradient)?;
    render_full(&s, &true_recipe(&s))
}

fn colour() -> Result<ImageF32, AppError> {
    let s = negative(2, Pattern::Blobs)?;
    let mut recipe = true_recipe(&s);
    recipe.color.red_cyan = 30.0;
    recipe.color.blue_yellow = -20.0;
    recipe.color.temperature = 40.0;
    recipe.color.tint = -25.0;
    recipe.color.saturation = 1.4;
    recipe.color.vibrance = 35.0;
    render_full(&s, &recipe)
}

fn tone() -> Result<ImageF32, AppError> {
    let s = negative(3, Pattern::Gradient)?;
    let mut recipe = true_recipe(&s);
    let t = &mut recipe.tone;
    t.exposure_ev = 0.5;
    t.contrast = 30.0;
    t.highlights = -40.0;
    t.shadows = 40.0;
    t.whites = 15.0;
    t.blacks = -15.0;
    t.dehaze = 25.0;
    let s_curve: Vec<CurvePoint> = vec![[0.0, 0.0], [0.25, 0.2], [0.75, 0.82], [1.0, 1.0]];
    t.curve.master = s_curve;
    t.curve.b = vec![[0.0, 0.03], [1.0, 0.97]];
    render_full(&s, &recipe)
}

fn mono_red_filter() -> Result<ImageF32, AppError> {
    let s = negative(4, Pattern::Blobs)?;
    let mut recipe = true_recipe(&s);
    recipe.mono.enabled = true;
    recipe.mono.mix = [0.8, 0.3, -0.1];
    render_full(&s, &recipe)
}

fn geometry() -> Result<ImageF32, AppError> {
    let s = negative(5, Pattern::Gradient)?;
    let mut recipe = true_recipe(&s);
    recipe.geometry.angle_deg = 3.5;
    recipe.geometry.rot90 = 1;
    recipe.geometry.flip_h = true;
    render_full(&s, &recipe)
}

fn auto() -> Result<ImageF32, AppError> {
    let s = negative(6, Pattern::Blobs)?;
    let input = PipelineInput {
        rgb: Arc::new(s.image.clone()),
        ir: None,
        film_base: None,
    };
    let analysis = analyze(&input)?;
    render(&input, &analysis.auto_recipe, RenderTarget::Full)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gtp-golden-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The committed goldens match the current pipeline. When a change is intended, run
    /// `cargo run -p gtp-benchmark -- golden --bless` and commit the new files.
    #[test]
    fn the_pipeline_matches_the_committed_goldens() {
        let outcomes = check(&default_dir(), None).unwrap();
        for o in &outcomes {
            assert!(
                o.passed(),
                "golden case {} differs: {:?} (limit max {MAX_DELTA_E}); \
                 if the change is intended, run `cargo run -p gtp-benchmark -- golden --bless`",
                o.name,
                o.diff
            );
        }
        assert_eq!(outcomes.len(), CASES.len());
    }

    #[test]
    fn a_changed_recipe_fails_its_golden() {
        let case = &CASES[0];
        let s = negative(1, Pattern::Gradient).unwrap();
        let mut recipe = true_recipe(&s);
        recipe.tone.exposure_ev = 0.1;
        let changed = stored_round_trip(&render_full(&s, &recipe).unwrap()).unwrap();
        let diff = compare_with_stored(case, &default_dir(), &changed, None)
            .unwrap()
            .unwrap();
        assert!(diff.max > MAX_DELTA_E, "{diff:?}");
    }

    #[test]
    fn bless_writes_goldens_that_then_pass_and_reports_the_change() {
        let dir = temp_dir("bless");
        let first = bless(&dir).unwrap();
        assert!(
            first.iter().all(|o| o.diff.is_none()),
            "new cases have no diff"
        );
        assert!(check(&dir, None).unwrap().iter().all(|o| o.diff
            == Some(ImageDiff {
                mean: 0.0,
                p95: 0.0,
                max: 0.0
            })));

        // Replace one golden with another case's render: check fails it and writes a map,
        // and bless reports the size of the change.
        let wrong = from_f32_linear(&scaled(&CASES[3].render().unwrap(), 1.0 / GOLDEN_SCALE));
        write_tiff16(&CASES[1].path_in(&dir), &wrong, Compression::None).unwrap();
        let maps = dir.join("maps");
        let checked = check(&dir, Some(&maps)).unwrap();
        assert!(!checked[1].passed() && checked[0].passed());
        assert!(maps.join("colour-delta-e.tif").exists());
        let again = bless(&dir).unwrap();
        assert!(again[1].diff.unwrap().max > MAX_DELTA_E);
        assert!(check(&dir, None).unwrap().iter().all(Outcome::passed));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_golden_fails() {
        let dir = temp_dir("missing");
        let outcomes = check(&dir, None).unwrap();
        assert!(outcomes.iter().all(|o| o.diff.is_none() && !o.passed()));
    }

    #[test]
    fn every_case_renders_a_plausible_image() {
        for case in CASES {
            let img = case.render().unwrap();
            assert_eq!(img.channels, 3, "{}", case.name);
            let mean = img.data.iter().sum::<f32>() / img.data.len() as f32;
            assert!((0.02..2.0).contains(&mean), "{}: mean {mean}", case.name);
        }
        let names: std::collections::HashSet<_> = CASES.iter().map(|c| c.name).collect();
        assert_eq!(names.len(), CASES.len(), "names are unique");
    }
}
