//! The reference report (Q-02): how far our automatic conversion of a camera triplet is from the
//! render of the same frame made by Trichroma Lab or Nikon Scan 4, in CIEDE2000.
//!
//! For each pair the triplet is merged and converted the way the app does at import (the 645 frame
//! window and the roll's film base), and our render of the whole capture is registered with the
//! reference ([`crate::register`]), so the two are compared over the same part of the picture.

use std::path::Path;
use std::sync::Arc;

use gtp_app::{
    analyze_with, merge_triplet, render, BaseEstimate, FrameHint, PipelineInput, RenderTarget,
};
use gtp_core::model::FILM_BASE_MIN_CONFIDENCE;
use gtp_core::recipe::{Crop, OutputSpace};
use gtp_core::{AppError, ErrorCode, FilmFormat, ImageF32};
use gtp_io::{encode_jpeg, read_tiff16, to_output, Limits, RawlerDecoder, Tiff16};

use crate::delta_e::{delta_e2000, rec2020_to_lab};
use crate::manifest::{Manifest, Pair};
use crate::register::{crop_resample, luma, register, Plane, Window};

/// Columns of the pictures that are compared.
pub const COMPARE_WIDTH: usize = 512;
/// The longest edge of our render of the whole capture.
const RENDER_EDGE: u32 = 1024;
/// A registration scoring below this is not trusted: the pair is probably not the same frame.
pub const MIN_SCORE: f32 = 0.8;
/// The ΔE a map's brightest pixel stands for.
const MAP_FULL_SCALE: f32 = 20.0;

/// What was measured on one pair.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameReport {
    pub name: String,
    /// The registration's correlation (0 to 1 for a match); `None` when the pair could not be
    /// registered at all (a flat picture).
    pub score: Option<f32>,
    /// The part of our capture the reference covers, as fractions of it.
    pub registered: Option<Window>,
    /// How far the crop the app detected is from it: the largest difference of the four edges, as a
    /// fraction of the capture.
    pub crop_gap: Option<f32>,
    /// ΔE2000 over the compared area; `None` when the pair was not registered well enough.
    pub median: Option<f32>,
    pub p95: Option<f32>,
    /// Our mean minus the reference's, in L*, a* and b*: a tone and colour cast the ΔE hides.
    pub cast: Option<[f32; 3]>,
}

/// The whole report.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub frames: Vec<FrameReport>,
    /// The film base the roll was converted with.
    pub film_base: [f32; 3],
    /// Over the pixels of all registered frames together.
    pub pooled_median: f32,
    pub pooled_p95: f32,
    /// The median of the frames' medians.
    pub median_of_medians: f32,
}

/// The value at `fraction` (0 to 1) of an ascending list by nearest rank; 0 for an empty list.
pub fn percentile(sorted: &[f32], fraction: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((sorted.len() as f32 * fraction).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

fn sorted(mut values: Vec<f32>) -> Vec<f32> {
    values.sort_unstable_by(f32::total_cmp);
    values
}

/// The film base a roll ends up with when `estimates` are measured on its frames in order, by the
/// rule of the app's import: the first is kept, unless it is a guess (below
/// [`FILM_BASE_MIN_CONFIDENCE`]) and a later one is not.
pub fn roll_base(estimates: &[BaseEstimate]) -> Option<BaseEstimate> {
    let mut base: Option<BaseEstimate> = None;
    for estimate in estimates {
        base = match base {
            None => Some(*estimate),
            Some(b)
                if b.confidence < FILM_BASE_MIN_CONFIDENCE
                    && estimate.confidence >= FILM_BASE_MIN_CONFIDENCE =>
            {
                Some(*estimate)
            }
            keep => keep,
        };
    }
    base
}

fn decode_srgb(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB-encoded values (0 to 1) as linear Rec.2020, the space the colour difference works in.
pub fn srgb_to_linear_rec2020(rgb: [f32; 3]) -> [f32; 3] {
    const M: [[f32; 3]; 3] = [
        [0.627_404, 0.329_283, 0.043_313],
        [0.069_097, 0.919_540, 0.011_362],
        [0.016_391, 0.088_013, 0.895_595],
    ];
    let [r, g, b] = rgb.map(decode_srgb);
    M.map(|row| row[0] * r + row[1] * g + row[2] * b)
}

/// A 16-bit sRGB TIFF shrunk to `out_w` columns by averaging, as sRGB-encoded floats from 0 to 1.
pub fn downscale_tiff(tiff: &Tiff16, out_w: usize) -> Result<ImageF32, AppError> {
    let channels = usize::from(tiff.channels);
    if channels < 3 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "A reference render needs three colour channels.",
        )
        .with_details(format!("{channels} channels")));
    }
    let (w, h) = (tiff.width as usize, tiff.height as usize);
    let out_w = out_w.clamp(1, w);
    let out_h = ((h * out_w) as f32 / w as f32).round().max(1.0) as usize;
    let mut data = Vec::with_capacity(out_w * out_h * 3);
    for oy in 0..out_h {
        let (y0, y1) = (oy * h / out_h, ((oy + 1) * h).div_ceil(out_h));
        for ox in 0..out_w {
            let (x0, x1) = (ox * w / out_w, ((ox + 1) * w).div_ceil(out_w));
            let mut sums = [0u64; 3];
            for y in y0..y1.max(y0 + 1) {
                for x in x0..x1.max(x0 + 1) {
                    let at = (y * w + x) * channels;
                    for (c, sum) in sums.iter_mut().enumerate() {
                        *sum += u64::from(tiff.data[at + c]);
                    }
                }
            }
            let count = ((y1.max(y0 + 1) - y0) * (x1.max(x0 + 1) - x0)) as f64;
            data.extend(sums.map(|s| (s as f64 / count / 65_535.0) as f32));
        }
    }
    ImageF32::new(out_w as u32, out_h as u32, 3, data)
}

/// Our render of the whole capture, as sRGB-encoded floats from 0 to 1.
fn srgb_floats(linear_rec2020: &ImageF32) -> Result<ImageF32, AppError> {
    let out = to_output(linear_rec2020, OutputSpace::Srgb)?;
    ImageF32::new(
        out.width,
        out.height,
        3,
        out.data.iter().map(|v| f32::from(*v) / 65_535.0).collect(),
    )
}

/// The ΔE2000 of every pixel of two sRGB-encoded images of the same size, and the mean difference
/// of L*, a* and b* (`a` minus `b`).
pub fn compare(a: &ImageF32, b: &ImageF32) -> Result<(Vec<f32>, [f32; 3]), AppError> {
    if (a.width, a.height, a.channels) != (b.width, b.height, b.channels) || a.channels != 3 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Only two 3-channel images of the same size can be compared.",
        ));
    }
    let (pa, _) = a.data.as_chunks::<3>();
    let (pb, _) = b.data.as_chunks::<3>();
    let mut des = Vec::with_capacity(pa.len());
    let mut sum = [0.0f64; 3];
    for (x, y) in pa.iter().zip(pb) {
        let (lx, ly) = (
            rec2020_to_lab(srgb_to_linear_rec2020(*x)),
            rec2020_to_lab(srgb_to_linear_rec2020(*y)),
        );
        des.push(delta_e2000(lx, ly));
        for c in 0..3 {
            sum[c] += f64::from(lx[c] - ly[c]);
        }
    }
    let n = pa.len().max(1) as f64;
    Ok((des, sum.map(|s| (s / n) as f32)))
}

fn crop_gap(registered: Window, ours: Crop) -> f32 {
    let edges = |x: f32, y: f32, w: f32, h: f32| [x, y, x + w, y + h];
    let a = edges(registered.x, registered.y, registered.w, registered.h);
    let b = edges(ours.x, ours.y, ours.w, ours.h);
    a.iter()
        .zip(b)
        .map(|(p, q)| (p - q).abs())
        .fold(0.0, f32::max)
}

fn merged(pair: &Pair) -> Result<ImageF32, AppError> {
    let [r, g, b] = &pair.exposures;
    Ok(merge_triplet(&RawlerDecoder, r, g, b, &Limits::default())?.image)
}

/// Writes ours, the reference and the ΔE (white at [`MAP_FULL_SCALE`]) side by side.
fn write_map(
    path: &Path,
    ours: &ImageF32,
    reference: &ImageF32,
    des: &[f32],
) -> Result<(), AppError> {
    let (w, h) = (ours.width as usize, ours.height as usize);
    let mut data = Vec::with_capacity(w * 3 * h * 3);
    let linear = |image: &ImageF32, i: usize| {
        srgb_to_linear_rec2020([
            image.data[i * 3],
            image.data[i * 3 + 1],
            image.data[i * 3 + 2],
        ])
    };
    for y in 0..h {
        for x in 0..w {
            data.extend(linear(ours, y * w + x));
        }
        for x in 0..w {
            data.extend(linear(reference, y * w + x));
        }
        for x in 0..w {
            let v = decode_srgb((des[y * w + x] / MAP_FULL_SCALE).clamp(0.0, 1.0));
            data.extend([v, v, v]);
        }
    }
    let image = ImageF32::new((w * 3) as u32, h as u32, 3, data)?;
    encode_jpeg(&image, OutputSpace::Srgb, 85, path)
}

/// Runs the report over a manifest. `note` hears of each step (for the command line to print);
/// `maps` is a folder to write a picture of each frame's difference to.
pub fn run(
    manifest: &Manifest,
    maps: Option<&Path>,
    note: &mut dyn FnMut(&str),
) -> Result<Report, AppError> {
    let hint = Some(FrameHint {
        aspect: manifest.format.aspect(),
        size: None,
    });
    let hint = hint.filter(|_| manifest.format != FilmFormat::Mm35);
    let analyze = |image: ImageF32, film_base: Option<[f32; 3]>| {
        analyze_with(
            &PipelineInput {
                rgb: Arc::new(image),
                ir: None,
                film_base,
            },
            hint,
        )
    };

    // The roll's base: given, or what the app's import ends with after measuring every frame.
    let film_base = match manifest.film_base {
        Some(base) => base,
        None => {
            let mut estimates = Vec::new();
            for pair in &manifest.pairs {
                note(&format!("measuring the film base on {}", pair.name));
                estimates.push(analyze(merged(pair)?, None)?.base);
            }
            roll_base(&estimates)
                .ok_or_else(|| AppError::internal("no film base was measured"))?
                .rgb
        }
    };
    note(&format!("film base {film_base:?}"));

    if let Some(dir) = maps {
        std::fs::create_dir_all(dir).map_err(|err| {
            AppError::new(ErrorCode::Io, "The map folder could not be made.")
                .with_details(err.to_string())
        })?;
    }

    let mut frames = Vec::new();
    let mut pooled = Vec::new();
    for pair in &manifest.pairs {
        note(&format!("converting {}", pair.name));
        let image = merged(pair)?;
        let analysis = analyze(image.clone(), Some(film_base))?;
        // The automatic recipe, but over the whole capture, so the reference can be found in it.
        let mut recipe = analysis.auto_recipe.clone();
        recipe.geometry.crop = Crop::default();
        let input = PipelineInput {
            rgb: Arc::new(image),
            ir: None,
            film_base: Some(film_base),
        };
        let full = srgb_floats(&render(
            &input,
            &recipe,
            RenderTarget::Preview {
                max_edge: RENDER_EDGE,
            },
        )?)?;
        let reference = downscale_tiff(
            &read_tiff16(&pair.reference, &Limits::default())?,
            COMPARE_WIDTH,
        )?;

        let reference_luma: Plane = luma(&reference);
        let found = register(&luma(&full), &reference_luma);
        let mut report = FrameReport {
            name: pair.name.clone(),
            score: found.map(|f| f.score),
            registered: found.map(|f| f.window),
            crop_gap: found.map(|f| crop_gap(f.window, analysis.frame.rect)),
            median: None,
            p95: None,
            cast: None,
        };
        if let Some(found) = found.filter(|f| f.score >= MIN_SCORE) {
            let ours = crop_resample(
                &full,
                found.window,
                reference.width as usize,
                reference.height as usize,
            )?;
            let (des, cast) = compare(&ours, &reference)?;
            let by_value = sorted(des.clone());
            report.median = Some(percentile(&by_value, 0.5));
            report.p95 = Some(percentile(&by_value, 0.95));
            report.cast = Some(cast);
            pooled.extend(des.iter().copied());
            if let Some(dir) = maps {
                write_map(
                    &dir.join(format!("{}.jpg", pair.name)),
                    &ours,
                    &reference,
                    &des,
                )?;
            }
        }
        frames.push(report);
    }

    let pooled = sorted(pooled);
    let medians = sorted(frames.iter().filter_map(|f| f.median).collect());
    Ok(Report {
        frames,
        film_base,
        pooled_median: percentile(&pooled, 0.5),
        pooled_p95: percentile(&pooled, 0.95),
        median_of_medians: percentile(&medians, 0.5),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estimate(confidence: f32) -> BaseEstimate {
        BaseEstimate {
            rgb: [confidence + 0.1, 0.5, 0.3],
            confidence,
        }
    }

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let values = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert_eq!(percentile(&values, 0.5), 5.0);
        assert_eq!(percentile(&values, 0.95), 10.0);
        assert_eq!(percentile(&values, 0.1), 1.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
        assert_eq!(percentile(&[7.0], 0.95), 7.0);
    }

    #[test]
    fn the_roll_base_is_the_first_unless_it_is_a_guess_and_a_later_one_is_not() {
        assert!(roll_base(&[]).is_none());
        let confident = roll_base(&[estimate(0.7), estimate(0.9)]).unwrap();
        assert_eq!(confident.confidence, 0.7, "a confident first base stays");
        let replaced = roll_base(&[estimate(0.0), estimate(0.34), estimate(0.6), estimate(0.9)]);
        assert_eq!(replaced.unwrap().confidence, 0.6);
        let all_guesses = roll_base(&[estimate(0.1), estimate(0.3)]).unwrap();
        assert_eq!(
            all_guesses.confidence, 0.1,
            "a guess does not replace a guess"
        );
    }

    #[test]
    fn srgb_white_and_black_map_to_the_rec2020_ends() {
        let white = srgb_to_linear_rec2020([1.0, 1.0, 1.0]);
        assert!(white.iter().all(|v| (v - 1.0).abs() < 1e-3), "{white:?}");
        assert_eq!(srgb_to_linear_rec2020([0.0; 3]), [0.0; 3]);
        // Grey stays grey: the three channels are equal.
        let grey = srgb_to_linear_rec2020([0.5, 0.5, 0.5]);
        assert!((grey[0] - grey[1]).abs() < 1e-3 && (grey[1] - grey[2]).abs() < 1e-3);
    }

    #[test]
    fn identical_pictures_differ_by_nothing_and_a_cast_shows_in_the_mean() {
        let pixels: Vec<f32> = (0..20 * 3).map(|i| (i % 7) as f32 / 8.0 + 0.05).collect();
        let a = ImageF32::new(5, 4, 3, pixels.clone()).unwrap();
        let (des, cast) = compare(&a, &a).unwrap();
        assert!(des.iter().all(|d| *d == 0.0));
        assert_eq!(cast, [0.0; 3]);

        let warmer = ImageF32::new(
            5,
            4,
            3,
            pixels
                .chunks(3)
                .flat_map(|p| [p[0] * 1.2, p[1], p[2] * 0.8])
                .collect(),
        )
        .unwrap();
        let (des, cast) = compare(&warmer, &a).unwrap();
        assert!(des.iter().any(|d| *d > 1.0));
        assert!(cast[1] > 0.0, "more red is a positive a*: {cast:?}");
        let small = ImageF32::new(2, 2, 3, vec![0.5; 12]).unwrap();
        assert!(compare(&a, &small).is_err());
    }

    #[test]
    fn a_tiff_is_averaged_down_to_the_wanted_width() {
        let tiff = Tiff16 {
            width: 4,
            height: 2,
            channels: 3,
            data: (0..4 * 2)
                .flat_map(|i| [i as u16 * 1000, 0, 65_535])
                .collect(),
        };
        let small = downscale_tiff(&tiff, 2).unwrap();
        assert_eq!((small.width, small.height), (2, 1));
        // The first output pixel averages source pixels 0, 1, 4 and 5.
        let want = (1000 + 4000 + 5000) as f32 / 4.0 / 65_535.0;
        assert!((small.data[0] - want).abs() < 1e-6);
        assert!((small.data[2] - 1.0).abs() < 1e-6);
        let gray = Tiff16 {
            channels: 1,
            data: vec![0; 8],
            ..tiff
        };
        assert!(downscale_tiff(&gray, 2).is_err());
    }

    #[test]
    fn the_crop_gap_is_the_largest_edge_difference() {
        let registered = Window {
            x: 0.08,
            y: 0.02,
            w: 0.87,
            h: 0.97,
        };
        let ours = Crop {
            x: 0.08,
            y: 0.02,
            w: 0.87,
            h: 0.97,
        };
        assert_eq!(crop_gap(registered, ours), 0.0);
        let off = Crop { x: 0.10, ..ours };
        assert!((crop_gap(registered, off) - 0.02).abs() < 1e-6);
    }
}
