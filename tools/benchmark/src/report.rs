//! The reference report (Q-02): how far our automatic conversion of a camera triplet is from the
//! render of the same frame made by Trichroma Lab or Nikon Scan 4, in CIEDE2000.
//!
//! For each pair the triplet is merged and converted the way the app does at import (the 645 frame
//! window and the roll's film base), and our render of the whole capture is registered with the
//! reference ([`crate::register`]), so the two are compared over the same part of the picture.

use std::path::Path;
use std::sync::Arc;

use gtp_app::{
    analyze_with, analyze_with_look, merge_triplet, render, BaseEstimate, FrameHint, PipelineInput,
    RenderTarget,
};
use gtp_core::model::FILM_BASE_MIN_CONFIDENCE;
use gtp_core::recipe::{Crop, OutputSpace};
use gtp_core::{AppError, ErrorCode, FilmFormat, ImageF32, Look};
use gtp_io::{
    encode_jpeg, read_tiff16, read_tiff16_single_strip, split_rgbi, to_f32_linear, to_output,
    Limits, RawlerDecoder, Tiff16,
};

use crate::delta_e::{delta_e2000, rec2020_to_lab};
use crate::manifest::{Manifest, Pair, Source};
use crate::register::{crop_resample, luma, register_oriented, Orientation, Window};

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
    /// The roll (or film stock) of the frame.
    pub roll: String,
    /// The registration's correlation (0 to 1 for a match); `None` when the pair could not be
    /// registered at all (a flat picture).
    pub score: Option<f32>,
    /// How the reference had to be turned or mirrored to lie as our render does.
    pub orientation: Option<Orientation>,
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
    /// Our mean chroma minus the reference's: positive when ours is the more saturated.
    pub chroma: Option<f32>,
}

/// What was measured on one roll (one film stock).
#[derive(Debug, Clone, PartialEq)]
pub struct RollReport {
    pub roll: String,
    /// The film base the roll was converted with.
    pub film_base: [f32; 3],
    /// Frames compared, of the frames the roll has.
    pub compared: usize,
    pub frames: usize,
    /// Over the pixels of the roll's compared frames together.
    pub median: f32,
    pub p95: f32,
}

/// The whole report.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub frames: Vec<FrameReport>,
    /// One entry for each roll, in the order the manifest first names them.
    pub rolls: Vec<RollReport>,
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
pub(crate) fn srgb_floats(linear_rec2020: &ImageF32) -> Result<ImageF32, AppError> {
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

/// Our mean chroma (C*) minus the reference's over two sRGB-encoded images of the same size: positive
/// when ours is the more saturated. 0 for two empty images.
pub fn mean_chroma_difference(a: &ImageF32, b: &ImageF32) -> f32 {
    let chroma = |image: &ImageF32| -> f64 {
        let (pixels, _) = image.data.as_chunks::<3>();
        let sum: f64 = pixels
            .iter()
            .map(|p| {
                let lab = rec2020_to_lab(srgb_to_linear_rec2020(*p));
                f64::from(lab[1]).hypot(f64::from(lab[2]))
            })
            .sum();
        sum / pixels.len().max(1) as f64
    };
    (chroma(a) - chroma(b)) as f32
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

/// The frame of a pair as linear RGB: the triplet merged, or the scan with its infrared plane (if it
/// has one) set aside.
pub(crate) fn load_frame(pair: &Pair) -> Result<ImageF32, AppError> {
    match &pair.source {
        Source::Triplet([r, g, b]) => {
            Ok(merge_triplet(&RawlerDecoder, r, g, b, &Limits::default())?.image)
        }
        Source::Scan(path) => {
            let tiff = read_tiff16(path, &Limits::default()).map_err(|err| {
                AppError::new(err.code, "A scan could not be read.").with_details(format!(
                    "{} ({}): {}",
                    pair.name,
                    path.display(),
                    err.message
                ))
            })?;
            let rgb = if tiff.channels == 4 {
                split_rgbi(&tiff)?.0
            } else {
                tiff
            };
            to_f32_linear(&rgb)
        }
    }
}

/// The reference of a pair, shrunk to [`COMPARE_WIDTH`] columns.
fn load_reference(pair: &Pair) -> Result<ImageF32, AppError> {
    load_reference_at(pair, COMPARE_WIDTH)
}

/// The reference of a pair shrunk to `width` columns. A file that cannot be read says which pair it
/// belongs to, and that a reference must be a 16-bit RGB TIFF.
pub(crate) fn load_reference_at(pair: &Pair, width: usize) -> Result<ImageF32, AppError> {
    let tiff = read_tiff16_single_strip(&pair.reference, &Limits::default()).map_err(|err| {
        AppError::new(
            err.code,
            "A reference render could not be read; it must be a 16-bit RGB TIFF.",
        )
        .with_details(format!(
            "{} ({}): {}",
            pair.name,
            pair.reference.display(),
            err.message
        ))
    })?;
    downscale_tiff(&tiff, width).map_err(|err| {
        AppError::new(err.code, err.message).with_details(format!(
            "{} ({})",
            pair.name,
            pair.reference.display()
        ))
    })
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

/// Runs the report over a manifest, converting every frame with the starting `look`. `note` hears of
/// each step (for the command line to print); `maps` is a folder to write a picture of each frame's
/// difference to.
pub fn run(
    manifest: &Manifest,
    maps: Option<&Path>,
    look: Look,
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

    // The rolls, in the order the manifest first names them, each with its film base: given, or
    // what the app's import ends with after measuring every frame of the roll.
    let mut rolls: Vec<&str> = Vec::new();
    for pair in &manifest.pairs {
        if !rolls.contains(&pair.roll.as_str()) {
            rolls.push(&pair.roll);
        }
    }
    let mut bases: Vec<[f32; 3]> = Vec::new();
    for roll in &rolls {
        let base = match manifest.film_base {
            Some(base) => base,
            None => {
                let mut estimates = Vec::new();
                for pair in manifest.pairs.iter().filter(|p| p.roll == *roll) {
                    note(&format!("measuring the film base on {}", pair.name));
                    estimates.push(analyze(load_frame(pair)?, None)?.base);
                }
                roll_base(&estimates)
                    .ok_or_else(|| AppError::internal("no film base was measured"))?
                    .rgb
            }
        };
        note(&format!("{roll}: film base {base:?}"));
        bases.push(base);
    }

    if let Some(dir) = maps {
        std::fs::create_dir_all(dir).map_err(|err| {
            AppError::new(ErrorCode::Io, "The map folder could not be made.")
                .with_details(err.to_string())
        })?;
    }

    let mut frames = Vec::new();
    let mut pooled_all = Vec::new();
    let mut pooled_by_roll: Vec<Vec<f32>> = vec![Vec::new(); rolls.len()];
    for pair in &manifest.pairs {
        note(&format!("converting {}", pair.name));
        let roll_index = rolls.iter().position(|r| *r == pair.roll).unwrap_or(0);
        let film_base = bases[roll_index];
        let image = load_frame(pair)?;
        let analysis = analyze_with_look(
            &PipelineInput {
                rgb: Arc::new(image.clone()),
                ir: None,
                film_base: Some(film_base),
            },
            hint,
            look,
        )?;
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
        let stored = load_reference(pair)?;

        // The reference may be turned or mirrored against our render (the LS-50 scans lie on their
        // side and Nikon Scan writes its positives upright): find how, then compare as it lies.
        let found = register_oriented(&luma(&full), &luma(&stored));
        let reference = match &found {
            Some((_, orientation)) => orientation.apply_image(&stored)?,
            None => stored,
        };
        let mut report = FrameReport {
            name: pair.name.clone(),
            roll: pair.roll.clone(),
            score: found.map(|(f, _)| f.score),
            orientation: found.map(|(_, o)| o),
            registered: found.map(|(f, _)| f.window),
            crop_gap: found.map(|(f, _)| crop_gap(f.window, analysis.frame.rect)),
            median: None,
            p95: None,
            cast: None,
            chroma: None,
        };
        if let Some((found, _)) = found.filter(|(f, _)| f.score >= MIN_SCORE) {
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
            report.chroma = Some(mean_chroma_difference(&ours, &reference));
            pooled_all.extend(des.iter().copied());
            pooled_by_roll[roll_index].extend(des.iter().copied());
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

    let roll_reports = rolls
        .iter()
        .zip(&bases)
        .zip(pooled_by_roll)
        .map(|((roll, base), pooled)| {
            let of_roll = || frames.iter().filter(|f| f.roll == *roll);
            let pooled = sorted(pooled);
            RollReport {
                roll: (*roll).to_owned(),
                film_base: *base,
                compared: of_roll().filter(|f| f.median.is_some()).count(),
                frames: of_roll().count(),
                median: percentile(&pooled, 0.5),
                p95: percentile(&pooled, 0.95),
            }
        })
        .collect();
    let pooled_all = sorted(pooled_all);
    let medians = sorted(frames.iter().filter_map(|f| f.median).collect());
    Ok(Report {
        frames,
        rolls: roll_reports,
        pooled_median: percentile(&pooled_all, 0.5),
        pooled_p95: percentile(&pooled_all, 0.95),
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

    /// A folder with a synthetic LS-50 scan and the reference made of our own render of it, turned a
    /// quarter turn clockwise as Nikon Scan's positives lie against the scans.
    fn scan_pair(label: &str) -> (std::path::PathBuf, Manifest, [f32; 3]) {
        use gtp_core::testing::synthetic::{self, Params, Pattern};
        use gtp_io::{from_f32_linear, write_tiff16, Compression};

        let dir = std::env::temp_dir().join(format!("gtp-report-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = synthetic::negative(
            3,
            &Params {
                width: 300,
                height: 200,
                rebate: [12; 4],
                pattern: Pattern::Blobs,
                ..Params::default()
            },
        )
        .unwrap();
        write_tiff16(
            &dir.join("scan.tif"),
            &from_f32_linear(&s.image),
            Compression::None,
        )
        .unwrap();

        // Our render of the whole scan, as the report makes it, turned and stored as 16 bits.
        let input = PipelineInput {
            rgb: Arc::new(s.image.clone()),
            ir: None,
            film_base: Some(s.truth.base),
        };
        let analysis = analyze_with(&input, None).unwrap();
        let mut recipe = analysis.auto_recipe;
        recipe.geometry.crop = Crop::default();
        let full = srgb_floats(
            &render(
                &input,
                &recipe,
                RenderTarget::Preview {
                    max_edge: RENDER_EDGE,
                },
            )
            .unwrap(),
        )
        .unwrap();
        let turned = Orientation {
            quarter_turns: 1,
            mirrored: false,
        }
        .apply_image(&full)
        .unwrap();
        let reference = gtp_io::Tiff16 {
            width: turned.width,
            height: turned.height,
            channels: 3,
            data: turned
                .data
                .iter()
                .map(|v| (v.clamp(0.0, 1.0) * 65_535.0).round() as u16)
                .collect(),
        };
        write_tiff16(&dir.join("positive.tif"), &reference, Compression::None).unwrap();

        let manifest = crate::manifest::parse(
            r#"{"format":"35mm","pairs":[
                {"name":"a","roll":"Stock A","scan":"scan.tif","reference":"positive.tif"}]}"#,
            &dir,
        )
        .unwrap();
        (dir, manifest, s.truth.base)
    }

    #[test]
    fn a_scan_pair_is_converted_turned_back_registered_and_compared() {
        let (dir, mut manifest, base) = scan_pair("scan");
        manifest.film_base = Some(base);
        let report = run(&manifest, None, Look::Neutral, &mut |_| {}).unwrap();
        let frame = &report.frames[0];
        assert_eq!(frame.roll, "Stock A");
        // The reference was stored a quarter turn clockwise, so it is turned three more to match.
        assert_eq!(
            frame.orientation,
            Some(Orientation {
                quarter_turns: 3,
                mirrored: false
            })
        );
        assert!(frame.score.unwrap() > 0.97, "{:?}", frame.score);
        // It is our own render, so the difference is only the resampling.
        assert!(frame.median.unwrap() < 1.0, "median {:?}", frame.median);
        assert_eq!(report.rolls.len(), 1);
        assert_eq!(report.rolls[0].roll, "Stock A");
        assert_eq!((report.rolls[0].compared, report.rolls[0].frames), (1, 1));
        assert_eq!(report.rolls[0].film_base, base);
        assert!(report.pooled_median < 1.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_film_base_is_measured_per_roll_when_it_is_not_given() {
        let (dir, manifest, _) = scan_pair("rolls");
        // The same scan and reference under two rolls: each roll measures its own base.
        let mut two = manifest.clone();
        let mut second = manifest.pairs[0].clone();
        second.name = "b".into();
        second.roll = "Stock B".into();
        two.pairs.push(second);
        let report = run(&two, None, Look::Neutral, &mut |_| {}).unwrap();
        let names: Vec<_> = report.rolls.iter().map(|r| r.roll.as_str()).collect();
        assert_eq!(names, ["Stock A", "Stock B"]);
        assert_eq!(report.rolls[0].film_base, report.rolls[1].film_base);
        assert_eq!(report.frames.len(), 2);
        assert!(report.frames.iter().all(|f| f.median.is_some()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reference_that_is_not_an_rgb_picture_names_its_pair() {
        use gtp_io::{write_tiff16, Compression, Tiff16};
        let (dir, manifest, base) = scan_pair("badref");
        let gray = Tiff16 {
            width: 8,
            height: 8,
            channels: 1,
            data: vec![1000; 64],
        };
        write_tiff16(&dir.join("positive.tif"), &gray, Compression::None).unwrap();
        let mut manifest = manifest;
        manifest.film_base = Some(base);
        let err = run(&manifest, None, Look::Neutral, &mut |_| {}).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let details = err.details.unwrap_or_default();
        assert!(details.contains("a ("), "the pair is named: {details}");
        assert!(
            details.contains("positive.tif"),
            "the file is named: {details}"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
