//! The proof of ADR 0015: the screen's conversion (`to_rgba8_srgb`) is within 1 level per channel of
//! what an sRGB export writes (`gtp_io::to_output` to sRGB, then `(v + 128) / 257` as the JPEG
//! writer takes 16 bits to 8). If the export's profiles or intent change, these fail first.
//!
//! The grids use cell centres, so they never sit on an exact rounding tie (a grid with 65 cells has
//! a colour at an encoded 0.1, 25.5 levels, where the two paths flip for no reason that matters).

use std::sync::Arc;

use gtp_core::recipe::OutputSpace;
use gtp_core::testing::synthetic::{self, Params, Pattern};
use gtp_io::to_output;

use super::*;
use crate::pipeline::{analyze, render, PipelineInput, RenderTarget};

/// The export's 8-bit values (RGB, 3 bytes a pixel).
fn export8(image: &ImageF32) -> Vec<u8> {
    to_output(image, OutputSpace::Srgb)
        .unwrap()
        .data
        .iter()
        .map(|&v| ((u32::from(v) + 128) / 257) as u8)
        .collect()
}

/// The screen's 8-bit values (RGB, the alpha dropped), checking that alpha is 255.
fn screen8(image: &ImageF32) -> Vec<u8> {
    let rgba = to_rgba8_srgb(image).unwrap();
    assert!(rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    rgba.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect()
}

/// The largest difference per channel between screen and export, and how many channels differ.
fn compare(image: &ImageF32) -> (u8, usize) {
    let (screen, export) = (screen8(image), export8(image));
    assert_eq!(screen.len(), export.len());
    let worst = screen
        .iter()
        .zip(&export)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    let differing = screen.iter().zip(&export).filter(|(a, b)| a != b).count();
    (worst, differing)
}

/// Asserts the 1-level agreement and that a difference at all is rare (the constants and the curve
/// are evaluated in a slightly different place; ADR 0015 measured at most 0.002 % on large sets).
fn assert_within_one_level(label: &str, image: &ImageF32) -> (u8, usize) {
    let (worst, differing) = compare(image);
    let channels = image.data.len();
    eprintln!("{label}: max {worst}, {differing} of {channels} channels differ");
    assert!(worst <= 1, "{label}: {worst} levels apart");
    assert!(
        differing * 200 <= channels,
        "{label}: {differing} of {channels} channels differ by a level"
    );
    (worst, differing)
}

fn image_of(points: &[[f32; 3]]) -> ImageF32 {
    let data: Vec<f32> = points.iter().flatten().copied().collect();
    ImageF32::new(points.len() as u32, 1, 3, data).unwrap()
}

/// `n` cell centres per axis between `lo` and `hi`.
fn grid(n: usize, lo: f32, hi: f32) -> Vec<[f32; 3]> {
    let f = |i: usize| lo + (hi - lo) * (i as f32 + 0.5) / n as f32;
    let mut out = Vec::with_capacity(n * n * n);
    for r in 0..n {
        for g in 0..n {
            for b in 0..n {
                out.push([f(r), f(g), f(b)]);
            }
        }
    }
    out
}

/// Linear sRGB primaries to linear Rec.2020 (the inverse of the screen's matrix, from the published
/// chromaticities).
const SRGB_TO_REC2020: [[f32; 3]; 3] = [
    [0.627_404, 0.329_283, 0.043_313],
    [0.069_097, 0.919_540, 0.011_362],
    [0.016_391, 0.088_013, 0.895_595],
];

fn matrix_row(m: &[[f32; 3]; 3], r: usize, p: &[f32; 3]) -> f32 {
    m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2]
}

/// Colours inside sRGB, evenly spaced in the sRGB encoding (so the dark end is covered too), as
/// linear Rec.2020.
fn in_gamut(n: usize) -> Vec<[f32; 3]> {
    grid(n, 0.0, 1.0)
        .into_iter()
        .map(|e| {
            let lin = e.map(|v| {
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            });
            [0, 1, 2].map(|r| matrix_row(&SRGB_TO_REC2020, r, &lin))
        })
        .collect()
}

fn outside_srgb(p: &[f32; 3]) -> bool {
    (0..3).any(|r| !(-0.001..=1.001).contains(&matrix_row(&REC2020_TO_SRGB, r, p)))
}

#[test]
fn colours_inside_srgb_match_the_export() {
    let points = in_gamut(24);
    assert_within_one_level("in-gamut grid", &image_of(&points));
}

#[test]
fn rec2020_colours_outside_srgb_match_the_export() {
    let points: Vec<[f32; 3]> = grid(24, 0.0, 1.0)
        .into_iter()
        .filter(outside_srgb)
        .collect();
    assert!(points.len() > 2_000, "the set must be large");
    assert_within_one_level("outside sRGB", &image_of(&points));
}

#[test]
fn values_above_one_match_the_export() {
    // The case the old order got wrong by up to 255 levels: a channel above 1 pulling the others
    // through the matrix's negative terms.
    let base = in_gamut(12);
    let mut points = Vec::new();
    for step in 0..7 {
        let k = 1.0 + 3.0 * step as f32 / 6.0;
        points.extend(base.iter().map(|p| p.map(|v| v * k)));
    }
    for p in grid(12, 0.0, 1.0) {
        points.push(p.map(|v| v * 4.0));
    }
    assert_within_one_level("up to 4x", &image_of(&points));
}

#[test]
fn near_black_and_near_white_match_the_export() {
    assert_within_one_level("near black", &image_of(&grid(17, 0.0, 0.01)));
    assert_within_one_level("near white", &image_of(&grid(17, 0.9, 1.0)));
}

#[test]
fn a_neutral_ramp_matches_the_export() {
    let ramp: Vec<[f32; 3]> = (0..=1024).map(|i| [i as f32 / 1024.0; 3]).collect();
    assert_within_one_level("neutral ramp", &image_of(&ramp));
}

#[test]
fn values_that_are_not_finite_or_negative_match_the_export() {
    let special = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -0.0,
        0.0,
        -3.0,
        0.5,
        1.0,
        7.0,
    ];
    // Every combination of the special values in the three channels.
    let mut points = Vec::new();
    for r in special {
        for g in special {
            for b in special {
                points.push([r, g, b]);
            }
        }
    }
    let image = image_of(&points);
    let (worst, _) = compare(&image);
    assert!(worst <= 1, "{worst} levels apart");
}

/// A synthetic negative rendered with a strongly saturated recipe, optionally brighter: most pixels
/// have a Rec.2020 channel above 1 and many leave sRGB.
fn strong_render(pattern: Pattern, seed: u64, exposure_ev: f32) -> ImageF32 {
    let (w, h) = (320u32, 213u32);
    let params = Params {
        width: w,
        height: h,
        rebate: [w / 14; 4],
        pattern,
        noise: 0.02,
        ..Params::default()
    };
    let scan = synthetic::negative(seed, &params).unwrap();
    let input = PipelineInput {
        rgb: Arc::new(scan.image),
        ir: None,
        film_base: None,
        full_long_edge: None,
    };
    let mut recipe = analyze(&input).unwrap().auto_recipe;
    recipe.color.saturation = 2.0;
    recipe.color.vibrance = 100.0;
    recipe.color.mixer.saturation = [100.0; 8];
    recipe.tone.exposure_ev = exposure_ev;
    render(&input, &recipe, RenderTarget::Preview { max_edge: 320 }).unwrap()
}

#[test]
fn strongly_saturated_renders_match_the_export() {
    for (pattern, seed) in [(Pattern::Gradient, 1), (Pattern::Blobs, 3)] {
        for ev in [0.0, 1.5] {
            let image = strong_render(pattern, seed, ev);
            let above_one = image
                .data
                .as_chunks::<3>()
                .0
                .iter()
                .filter(|p| p.iter().any(|v| *v > 1.0))
                .count();
            assert!(
                above_one * 10 > image.data.len() / 3,
                "{pattern:?} at {ev} EV: too few values above 1 to prove anything"
            );
            assert_within_one_level(&format!("{pattern:?} at {ev} EV"), &image);
        }
    }
}

#[test]
fn the_histogram_of_a_render_is_built_from_the_pixels_of_the_new_conversion() {
    // The cumulative count of each channel at level k lies between the export's counts at k - 1
    // and k + 1, which a histogram of the old order did not (it was up to 255 levels off).
    let image = strong_render(Pattern::Gradient, 1, 1.5);
    let screen = histogram(&to_rgba8_srgb(&image).unwrap());
    let export = export8(&image);
    let mut file = [[0u32; 256]; 3];
    for p in export.as_chunks::<3>().0 {
        for (c, v) in p.iter().enumerate() {
            file[c][usize::from(*v)] += 1;
        }
    }
    let cumulative = |bins: &[u32; 256], k: i32| -> u32 {
        if k < 0 {
            0
        } else {
            bins[..=(k.min(255) as usize)].iter().sum()
        }
    };
    for (c, shown) in [&screen.red, &screen.green, &screen.blue]
        .into_iter()
        .enumerate()
    {
        for k in 0..256 {
            let s = cumulative(shown, k);
            assert!(s <= cumulative(&file[c], k + 1), "channel {c} level {k}");
            assert!(s >= cumulative(&file[c], k - 1), "channel {c} level {k}");
        }
    }
    // Some pixels are clipped to white, so the check above covers the overlay's rule too.
    assert!(screen.red[255] + screen.green[255] + screen.blue[255] > 0);
}

#[test]
fn the_analysis_histogram_is_the_histogram_of_to_rgba8_srgb() {
    let scan = synthetic::negative(
        8,
        &Params {
            width: 160,
            height: 107,
            rebate: [12; 4],
            pattern: Pattern::Blobs,
            ..Params::default()
        },
    )
    .unwrap();
    let input = PipelineInput {
        rgb: Arc::new(scan.image),
        ir: None,
        film_base: None,
        full_long_edge: None,
    };
    let analysis = analyze(&input).unwrap();
    // The scan is smaller than the proxy edge, so analyze renders the scan itself.
    let image = render(&input, &analysis.auto_recipe, RenderTarget::Full).unwrap();
    assert_eq!(
        analysis.histogram,
        histogram(&to_rgba8_srgb(&image).unwrap())
    );
}
