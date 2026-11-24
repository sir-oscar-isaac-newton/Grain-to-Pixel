//! Inversion in the density domain (docs/specification.md section 4.2 step 5).
//!
//! The input is a scan divided by the film base ([`crate::normalize`]), so every value is a
//! transmittance `T` with the unexposed base at 1.0. Per colour channel:
//!
//! ```text
//! D = -log10(T)                          density; T is floored at 1e-6
//! P = max(0, (D - black) / (white - black))
//! ```
//!
//! `black` and `white` are the densities that map to 0 and to 1; they are the `black` and `white`
//! of the Recipe's `invert` block. `P` is linear in density: the display curve and the film's
//! contrast are the job of the tone stage, which is calibrated against reference renders. There
//! is no upper clamp, so highlights keep headroom for the exposure slider; the output stage clips.
//!
//! [`auto_levels`] estimates `black` and `white` from percentiles of the density and returns a
//! plain `Invert` that can be stored in the Recipe; values the user set are used as they are
//! ([`levels_or_auto`]).
//!
//! A frame of a black-and-white roll carries one picture in three channels, so its `Invert` may
//! carry `combine` weights (ADR 0017). The three densities are then combined into one,
//! `D = sum(w_c * D_c) / sum(w_c)`, and one pair of points (the mean of the three `black` values
//! and of the three `white` values) maps it to `P`, which is written to all three channels:
//!
//! ```text
//! P = max(0, (D - black) / (white - black))      the same value in R, G and B
//! ```
//!
//! [`combine_weights`] finds the weights from the scan's own noise (inverse variance, so the
//! least noisy channels count most), [`auto_levels_combined`] finds the weights and the points.
//! Without `combine` nothing here changes: each channel is inverted on its own.

use gtp_core::recipe::{Invert, INVERT_GAP, INVERT_MAX};
use gtp_core::{AppError, ErrorCode, ImageF32};
use rayon::prelude::*;

use crate::film_base::{channel_samples, check_rect, median, Rect};

/// Default low percentile for the black point.
pub const DEFAULT_LOW_PERCENTILE: f32 = 0.5;
/// Default high percentile for the white point.
pub const DEFAULT_HIGH_PERCENTILE: f32 = 99.5;

/// Transmittance floor: the largest density that can occur is `-log10(1e-6) = 6`.
const TRANSMITTANCE_FLOOR: f32 = 1e-6;
/// `auto_levels` looks at about this many pixels at most, on a regular grid.
const MAX_LEVEL_SAMPLES: u64 = 1_000_000;

fn invalid(message: &str, details: String) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message).with_details(details)
}

fn require_rgb(image: &ImageF32) -> Result<(), AppError> {
    if image.channels == 3 {
        Ok(())
    } else {
        Err(invalid(
            "Inversion needs a 3-channel image.",
            format!("{} channels", image.channels),
        ))
    }
}

/// Density of a transmittance. A sample that is not finite counts as the film base (density 0).
fn density(transmittance: f32) -> f32 {
    let t = if transmittance.is_finite() {
        transmittance.max(TRANSMITTANCE_FLOOR)
    } else {
        1.0
    };
    -t.log10()
}

/// Maps densities to positive values with the given black and white points, in place.
/// Fails if a point is not finite or `white` is not above `black` in any channel.
pub fn invert(image: &mut ImageF32, levels: &Invert) -> Result<(), AppError> {
    require_rgb(image)?;
    if let Some(weights) = levels.combine {
        return invert_combined(image, levels, weights);
    }
    let span = [0, 1, 2].map(|c| levels.white[c] - levels.black[c]);
    let usable =
        (0..3).all(|c| levels.black[c].is_finite() && levels.white[c].is_finite() && span[c] > 0.0);
    if !usable {
        return Err(invalid(
            "White must be a higher density than black.",
            format!("black {:?}, white {:?}", levels.black, levels.white),
        ));
    }
    let (pixels, _) = image.data.as_chunks_mut::<3>();
    pixels.par_iter_mut().for_each(|pixel| {
        for (c, sample) in pixel.iter_mut().enumerate() {
            *sample = ((density(*sample) - levels.black[c]) / span[c]).max(0.0);
        }
    });
    Ok(())
}

/// The weights of a combined inversion, checked: every one finite and at least 0, with a positive
/// sum (which is returned). A recipe that went through `Recipe::clamped` always passes.
fn check_weights(weights: [f32; 3]) -> Result<f32, AppError> {
    let sum: f32 = weights.iter().sum();
    if weights.iter().all(|w| w.is_finite() && *w >= 0.0) && sum.is_finite() && sum > 0.0 {
        Ok(sum)
    } else {
        Err(invalid(
            "The channel weights must be positive numbers.",
            format!("{weights:?}"),
        ))
    }
}

/// The combined density of three channel densities; `total` is the sum of the weights.
fn combined_density(densities: [f32; 3], weights: &[f32; 3], total: f32) -> f32 {
    let weighted: f32 = densities.iter().zip(weights).map(|(d, w)| w * d).sum();
    weighted / total
}

/// The combined inversion: one density from the three channels, one pair of points (the means of
/// the recipe's three black and three white values), the result in all three channels.
fn invert_combined(
    image: &mut ImageF32,
    levels: &Invert,
    weights: [f32; 3],
) -> Result<(), AppError> {
    let total = check_weights(weights)?;
    let black = levels.black.iter().sum::<f32>() / 3.0;
    let white = levels.white.iter().sum::<f32>() / 3.0;
    if !(black.is_finite() && white.is_finite() && white > black) {
        return Err(invalid(
            "White must be a higher density than black.",
            format!("black {:?}, white {:?}", levels.black, levels.white),
        ));
    }
    let span = white - black;
    let (pixels, _) = image.data.as_chunks_mut::<3>();
    pixels.par_iter_mut().for_each(|pixel| {
        let d = combined_density(pixel.map(density), &weights, total);
        *pixel = [((d - black) / span).max(0.0); 3];
    });
    Ok(())
}

/// The value at `percent` (0 to 100) of the sorted `values`; reorders them. Not for empty slices.
fn percentile(values: &mut [f32], percent: f32) -> f32 {
    let index = ((percent / 100.0) * (values.len() - 1) as f32).round() as usize;
    let (_, value, _) = values.select_nth_unstable_by(index.min(values.len() - 1), f32::total_cmp);
    *value
}

fn check_percentiles(low_percent: f32, high_percent: f32) -> Result<(), AppError> {
    let ordered = low_percent.is_finite()
        && high_percent.is_finite()
        && 0.0 <= low_percent
        && low_percent < high_percent
        && high_percent <= 100.0;
    if ordered {
        Ok(())
    } else {
        Err(invalid(
            "The percentiles must satisfy 0 <= low < high <= 100.",
            format!("{low_percent}, {high_percent}"),
        ))
    }
}

/// The step of the regular grid the automatic estimates look at: about [`MAX_LEVEL_SAMPLES`]
/// pixels at most.
fn sample_step(image: &ImageF32) -> usize {
    let pixels = u64::from(image.width) * u64::from(image.height);
    ((pixels as f64 / MAX_LEVEL_SAMPLES as f64).sqrt().ceil() as usize).max(1)
}

/// Estimates the black and white points of a normalized scan: per channel, the densities at the
/// `low_percent` and `high_percent` percentiles. Pass the cropped frame, not the rebate, which
/// would otherwise pull the black point down. The result is within the Recipe's ranges.
pub fn auto_levels(
    image: &ImageF32,
    low_percent: f32,
    high_percent: f32,
) -> Result<Invert, AppError> {
    require_rgb(image)?;
    check_percentiles(low_percent, high_percent)?;
    let step = sample_step(image);
    let width = image.width as usize;
    let mut channels: [Vec<f32>; 3] = Default::default();
    for y in (0..image.height as usize).step_by(step) {
        for x in (0..width).step_by(step) {
            let at = (y * width + x) * 3;
            for (c, channel) in channels.iter_mut().enumerate() {
                match image.data.get(at + c) {
                    Some(t) if t.is_finite() => channel.push(density(*t)),
                    _ => {}
                }
            }
        }
    }

    let mut levels = Invert::default();
    for (c, samples) in channels.iter_mut().enumerate() {
        if samples.is_empty() {
            return Err(invalid(
                "The image has no usable pixels to measure.",
                format!("channel {c}"),
            ));
        }
        let black = percentile(samples, low_percent).clamp(0.0, INVERT_MAX - INVERT_GAP);
        let white = percentile(samples, high_percent).clamp(0.0, INVERT_MAX);
        levels.black[c] = black;
        levels.white[c] = white.max(black + INVERT_GAP);
    }
    Ok(levels)
}

/// The densities of the pixel at index `at` of the data when all three channels are finite; `None`
/// otherwise.
fn finite_densities(image: &ImageF32, at: usize) -> Option<[f32; 3]> {
    let pixel = <&[f32; 3]>::try_from(image.data.get(at..at + 3)?).ok()?;
    pixel
        .iter()
        .all(|t| t.is_finite())
        .then(|| pixel.map(density))
}

/// Scale factor from the median absolute deviation to the standard deviation of a normal
/// distribution.
const MAD_TO_SIGMA: f32 = 1.4826;

/// The weights of the three channels' densities in the combined density of a black-and-white
/// frame, adding up to 1. With `D_c = a_c * S + noise_c` (one picture `S` seen through three
/// channels, `a_c` proportional to the channel's signal span), the combination with the least
/// noise for the signal it carries weights channel `c` by `span_c / sigma_c^2`:
///
/// - `sigma_c` is the standard deviation of the channel's density noise, from the median absolute
///   deviation of the density differences between horizontal neighbours (divided by the square
///   root of two, the noise of a difference of two samples), which picture detail hardly moves;
/// - `span_c` is the channel's signal span, its 99.5th minus its 0.5th percentile density.
///
/// Both come from a regular grid of about a million pixels. When a channel's noise or span is not
/// finite and above 0 (a noiseless, flat or degenerate image), or the image has too few usable
/// pixels, the weights are equal. Pass the cropped, normalized frame.
pub fn combine_weights(image: &ImageF32) -> Result<[f32; 3], AppError> {
    require_rgb(image)?;
    let equal = [1.0 / 3.0; 3];
    let step = sample_step(image);
    let width = image.width as usize;
    let mut densities: [Vec<f32>; 3] = Default::default();
    let mut differences: [Vec<f32>; 3] = Default::default();
    for y in (0..image.height as usize).step_by(step) {
        for x in (0..width).step_by(step) {
            let at = (y * width + x) * 3;
            let Some(here) = finite_densities(image, at) else {
                continue;
            };
            for (channel, d) in densities.iter_mut().zip(here) {
                channel.push(d);
            }
            if x + 1 < width {
                if let Some(next) = finite_densities(image, at + 3) {
                    for (c, channel) in differences.iter_mut().enumerate() {
                        channel.push(next[c] - here[c]);
                    }
                }
            }
        }
    }

    let mut weights = [0.0f32; 3];
    for c in 0..3 {
        let Some(centre) = median(&mut differences[c]) else {
            return Ok(equal);
        };
        let mut deviations: Vec<f32> = differences[c].iter().map(|d| (d - centre).abs()).collect();
        let Some(mad) = median(&mut deviations) else {
            return Ok(equal);
        };
        let sigma = MAD_TO_SIGMA * mad / std::f32::consts::SQRT_2;
        let span = percentile(&mut densities[c], DEFAULT_HIGH_PERCENTILE)
            - percentile(&mut densities[c], DEFAULT_LOW_PERCENTILE);
        if !(sigma.is_finite() && sigma > 0.0 && span.is_finite() && span > 0.0) {
            return Ok(equal);
        }
        weights[c] = span / (sigma * sigma);
    }
    let sum: f32 = weights.iter().sum();
    if weights.iter().all(|w| w.is_finite()) && sum.is_finite() && sum > 0.0 {
        Ok(weights.map(|w| w / sum))
    } else {
        Ok(equal)
    }
}

/// Estimates the combined inversion of a normalized black-and-white frame: the weights from
/// [`combine_weights`], and one black and one white point from the `low_percent` and
/// `high_percent` percentiles of the combined density, written to all three channels. The result
/// is within the Recipe's ranges. Pass the cropped frame, not the rebate. Pixels with a channel
/// that is not finite are left out.
pub fn auto_levels_combined(
    image: &ImageF32,
    low_percent: f32,
    high_percent: f32,
) -> Result<Invert, AppError> {
    require_rgb(image)?;
    check_percentiles(low_percent, high_percent)?;
    let weights = combine_weights(image)?;
    let total = check_weights(weights)?;
    let step = sample_step(image);
    let width = image.width as usize;
    let mut combined = Vec::new();
    for y in (0..image.height as usize).step_by(step) {
        for x in (0..width).step_by(step) {
            if let Some(d) = finite_densities(image, (y * width + x) * 3) {
                combined.push(combined_density(d, &weights, total));
            }
        }
    }
    if combined.is_empty() {
        return Err(invalid(
            "The image has no usable pixels to measure.",
            "combined density".to_string(),
        ));
    }
    let black = percentile(&mut combined, low_percent).clamp(0.0, INVERT_MAX - INVERT_GAP);
    let white = percentile(&mut combined, high_percent).clamp(0.0, INVERT_MAX);
    Ok(Invert {
        black: [black; 3],
        white: [white.max(black + INVERT_GAP); 3],
        combine: Some(weights),
    })
}

/// Which end of the levels a picked area sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelPoint {
    /// The density that maps to 1.
    White,
    /// The density that maps to 0.
    Black,
}

/// Sets the white or the black point of `current` from an area of a normalized scan: per channel,
/// the density of the area's median. The other end is kept, and white stays above black by at
/// least `INVERT_GAP` (the end being set gives way). An area with no finite pixels, or whose
/// median in a channel is not above zero (black), is rejected.
///
/// When `current` is a combined inversion (`combine` set) the point is the density of the median of
/// the area's combined density, written to all three channels, and the other end is made equal in
/// all three channels as well (at its mean, which is the value the inversion uses); the gap rules
/// are the same, applied to those means.
pub fn levels_from_sample(
    image: &ImageF32,
    area: Rect,
    point: LevelPoint,
    current: &Invert,
) -> Result<Invert, AppError> {
    require_rgb(image)?;
    check_rect(image, area)?;
    let pixels = u64::from(area.w) * u64::from(area.h);
    let step = ((pixels as f64 / MAX_LEVEL_SAMPLES as f64).sqrt().ceil() as u32).max(1);
    if let Some(weights) = current.combine {
        return combined_levels_from_sample(image, area, step, point, current, weights);
    }
    let mut medians = [0.0f32; 3];
    for (c, mut samples) in channel_samples(image, area, step).into_iter().enumerate() {
        let m = median(&mut samples)
            .ok_or_else(|| invalid("That area has no usable pixels.", format!("{area:?}")))?;
        if m <= 0.0 {
            return Err(invalid(
                "That area is black, so it says nothing about the levels.",
                format!("channel {c}, median {m}"),
            ));
        }
        medians[c] = density(m);
    }

    let mut levels = *current;
    for (c, d) in medians.into_iter().enumerate() {
        match point {
            LevelPoint::White => {
                levels.white[c] = d.clamp(0.0, INVERT_MAX).max(levels.black[c] + INVERT_GAP);
            }
            LevelPoint::Black => {
                let top = (levels.white[c] - INVERT_GAP).clamp(0.0, INVERT_MAX - INVERT_GAP);
                levels.black[c] = d.clamp(0.0, top);
                levels.white[c] = levels.white[c].max(levels.black[c] + INVERT_GAP);
            }
        }
    }
    Ok(levels)
}

/// [`levels_from_sample`] for a combined inversion.
fn combined_levels_from_sample(
    image: &ImageF32,
    area: Rect,
    step: u32,
    point: LevelPoint,
    current: &Invert,
    weights: [f32; 3],
) -> Result<Invert, AppError> {
    let total = check_weights(weights)?;
    let width = image.width as usize;
    let mut transmittances: [Vec<f32>; 3] = Default::default();
    let mut combined = Vec::new();
    for y in (area.y..area.y + area.h).step_by(step as usize) {
        for x in (area.x..area.x + area.w).step_by(step as usize) {
            let at = (y as usize * width + x as usize) * 3;
            let Some(d) = finite_densities(image, at) else {
                continue;
            };
            for (channel, t) in transmittances.iter_mut().zip(&image.data[at..at + 3]) {
                channel.push(*t);
            }
            combined.push(combined_density(d, &weights, total));
        }
    }
    for (c, samples) in transmittances.iter_mut().enumerate() {
        let m = median(samples)
            .ok_or_else(|| invalid("That area has no usable pixels.", format!("{area:?}")))?;
        if m <= 0.0 {
            return Err(invalid(
                "That area is black, so it says nothing about the levels.",
                format!("channel {c}, median {m}"),
            ));
        }
    }
    let d = median(&mut combined)
        .ok_or_else(|| invalid("That area has no usable pixels.", format!("{area:?}")))?;

    let black = current.black.iter().sum::<f32>() / 3.0;
    let white = current.white.iter().sum::<f32>() / 3.0;
    let (black, white) = match point {
        LevelPoint::White => (black, d.clamp(0.0, INVERT_MAX).max(black + INVERT_GAP)),
        LevelPoint::Black => {
            let top = (white - INVERT_GAP).clamp(0.0, INVERT_MAX - INVERT_GAP);
            let black = d.clamp(0.0, top);
            (black, white.max(black + INVERT_GAP))
        }
    };
    Ok(Invert {
        black: [black; 3],
        white: [white; 3],
        combine: current.combine,
    })
}

/// The levels to invert with: the user's `explicit` values if there are any, otherwise the
/// automatic ones from [`auto_levels`].
pub fn levels_or_auto(
    image: &ImageF32,
    explicit: Option<&Invert>,
    low_percent: f32,
    high_percent: f32,
) -> Result<Invert, AppError> {
    match explicit {
        Some(levels) => Ok(*levels),
        None => auto_levels(image, low_percent, high_percent),
    }
}

#[cfg(test)]
mod tests {
    use gtp_core::testing::synthetic::{self, Params, Pattern, Synthetic};
    use gtp_core::Recipe;
    use proptest::array::uniform3;
    use proptest::prelude::*;

    use super::*;
    use crate::normalize;

    fn scan(pattern: Pattern, noise: f32) -> Synthetic {
        synthetic::negative(
            7,
            &Params {
                pattern,
                noise,
                ..Params::default()
            },
        )
        .unwrap()
    }

    fn normalized(s: &Synthetic) -> ImageF32 {
        let mut image = s.image.clone();
        normalize(&mut image, s.truth.base).unwrap();
        image
    }

    /// The frame area of an image, as its own image.
    fn crop_to_frame(s: &Synthetic, image: &ImageF32) -> ImageF32 {
        let f = s.truth.frame;
        let mut data = Vec::new();
        for y in f.y..f.y + f.h {
            let row = image.row(y).unwrap();
            data.extend_from_slice(&row[f.x as usize * 3..(f.x + f.w) as usize * 3]);
        }
        ImageF32::new(f.w, f.h, 3, data).unwrap()
    }

    fn levels(black: f32, white: f32) -> Invert {
        Invert {
            black: [black; 3],
            white: [white; 3],
            combine: None,
        }
    }

    #[test]
    fn inverting_with_the_true_levels_recovers_the_expected_positive() {
        for pattern in [Pattern::Wedge, Pattern::Gradient, Pattern::Blobs] {
            let s = scan(pattern, 0.0);
            let mut image = normalized(&s);
            let truth = Invert {
                black: s.truth.black,
                white: s.truth.white,
                combine: None,
            };
            invert(&mut image, &truth).unwrap();
            for (i, (got, want)) in image
                .data
                .iter()
                .zip(&s.truth.expected_positive.data)
                .enumerate()
            {
                assert!(
                    (got - want).abs() < 1e-3,
                    "{pattern:?} sample {i}: {got} vs {want}"
                );
            }
        }
    }

    /// Value at `percent` of a channel of the expected positive inside the frame, using the same
    /// index rule as `auto_levels`.
    fn truth_percentile(s: &Synthetic, channel: usize, percent: f32) -> f32 {
        let frame = crop_to_frame(s, &s.truth.expected_positive);
        let mut values: Vec<f32> = frame
            .data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p[channel])
            .collect();
        percentile(&mut values, percent)
    }

    #[test]
    fn auto_levels_stretches_the_frame_between_its_percentiles() {
        for pattern in [Pattern::Wedge, Pattern::Gradient, Pattern::Blobs] {
            let s = scan(pattern, 0.0);
            let frame = crop_to_frame(&s, &normalized(&s));
            let auto =
                auto_levels(&frame, DEFAULT_LOW_PERCENTILE, DEFAULT_HIGH_PERCENTILE).unwrap();
            let mut inverted = frame.clone();
            invert(&mut inverted, &auto).unwrap();

            let positive = crop_to_frame(&s, &s.truth.expected_positive);
            for c in 0..3 {
                let (lo, hi) = (
                    truth_percentile(&s, c, DEFAULT_LOW_PERCENTILE),
                    truth_percentile(&s, c, DEFAULT_HIGH_PERCENTILE),
                );
                let (got_pixels, _) = inverted.data.as_chunks::<3>();
                let (want_pixels, _) = positive.data.as_chunks::<3>();
                for (got, p) in got_pixels.iter().zip(want_pixels) {
                    let want = ((p[c] - lo) / (hi - lo)).max(0.0);
                    assert!(
                        (got[c] - want).abs() < 5e-3,
                        "{pattern:?} channel {c}: {} vs {want}",
                        got[c]
                    );
                }
            }
        }
    }

    #[test]
    fn on_a_wedge_the_automatic_points_are_the_true_ones() {
        let s = scan(Pattern::Wedge, 0.0);
        let frame = crop_to_frame(&s, &normalized(&s));
        let auto = auto_levels(&frame, DEFAULT_LOW_PERCENTILE, DEFAULT_HIGH_PERCENTILE).unwrap();
        for c in 0..3 {
            assert!((auto.black[c] - s.truth.black[c]).abs() < 0.01, "black {c}");
            assert!((auto.white[c] - s.truth.white[c]).abs() < 0.01, "white {c}");
        }
    }

    #[test]
    fn auto_levels_ignore_hot_pixels_that_are_rarer_than_the_trim() {
        let s = scan(Pattern::Wedge, 0.0);
        let clean = crop_to_frame(&s, &normalized(&s));
        let mut dirty = clean.clone();
        let pixels = dirty.data.len() / 3;
        for i in (0..pixels).step_by(500) {
            dirty.data[i * 3..i * 3 + 3].fill(1e-9); // 0.2% hot pixels: far denser than any film
        }
        let a = auto_levels(&clean, 0.5, 99.5).unwrap();
        let b = auto_levels(&dirty, 0.5, 99.5).unwrap();
        for c in 0..3 {
            assert!(
                (a.white[c] - b.white[c]).abs() < 0.05,
                "white {c}: {} vs {}",
                a.white[c],
                b.white[c]
            );
            assert_eq!(a.black[c], b.black[c]);
        }
    }

    #[test]
    fn auto_levels_stay_inside_the_recipe_ranges_for_odd_images() {
        let flat = ImageF32::new(8, 8, 3, vec![0.5; 192]).unwrap();
        let dark = ImageF32::new(8, 8, 3, vec![0.0; 192]).unwrap();
        let bright = ImageF32::new(8, 8, 3, vec![1.0e9; 192]).unwrap();
        for image in [flat, dark, bright] {
            let auto = auto_levels(&image, 0.5, 99.5).unwrap();
            let recipe = Recipe {
                invert: auto,
                ..Recipe::default()
            };
            assert_eq!(
                recipe.clone().clamped().invert,
                auto,
                "already in range: {auto:?}"
            );
            for c in 0..3 {
                assert!(auto.white[c] >= auto.black[c] + INVERT_GAP);
            }
        }
    }

    #[test]
    fn explicit_levels_override_the_automatic_ones() {
        let s = scan(Pattern::Gradient, 0.0);
        let frame = crop_to_frame(&s, &normalized(&s));
        let explicit = levels(0.2, 1.7);
        assert_eq!(
            levels_or_auto(&frame, Some(&explicit), 0.5, 99.5).unwrap(),
            explicit
        );
        let auto = levels_or_auto(&frame, None, 0.5, 99.5).unwrap();
        assert_eq!(auto, auto_levels(&frame, 0.5, 99.5).unwrap());
        assert_ne!(auto, explicit);
    }

    #[test]
    fn the_base_inverts_to_black_and_a_denser_pixel_is_brighter() {
        let mut image = ImageF32::new(
            3,
            1,
            3,
            vec![1.0, 1.0, 1.0, 0.1, 0.1, 0.1, 0.01, 0.01, 0.01],
        )
        .unwrap();
        invert(&mut image, &levels(0.0, 2.0)).unwrap();
        assert_eq!(&image.data[..3], [0.0; 3], "the film base is density 0");
        assert!((image.data[3] - 0.5).abs() < 1e-6, "T=0.1 is density 1");
        assert!((image.data[6] - 1.0).abs() < 1e-6, "T=0.01 is density 2");
    }

    #[test]
    fn odd_samples_stay_finite_and_never_negative() {
        let mut image = ImageF32::new(
            2,
            1,
            3,
            vec![
                f32::NAN,
                f32::INFINITY,
                -1.0,
                0.0,
                f32::NEG_INFINITY,
                1.0e30,
            ],
        )
        .unwrap();
        invert(&mut image, &levels(0.5, 3.0)).unwrap();
        assert!(
            image.data.iter().all(|v| v.is_finite() && *v >= 0.0),
            "{:?}",
            image.data
        );
    }

    #[test]
    fn bad_input_is_invalid_input() {
        let rgb = ImageF32::zeros(4, 4, 3).unwrap();
        let gray = ImageF32::zeros(4, 4, 1).unwrap();
        let mut copy = rgb.clone();
        for bad in [
            levels(1.0, 1.0),
            levels(2.0, 1.0),
            levels(f32::NAN, 1.0),
            levels(0.0, f32::INFINITY),
        ] {
            assert_eq!(
                invert(&mut copy, &bad).unwrap_err().code,
                ErrorCode::InvalidInput
            );
        }
        assert_eq!(
            invert(&mut gray.clone(), &levels(0.0, 1.0))
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
        assert_eq!(
            auto_levels(&gray, 0.5, 99.5).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        for (lo, hi) in [
            (50.0, 50.0),
            (60.0, 40.0),
            (-1.0, 50.0),
            (0.0, 101.0),
            (f32::NAN, 50.0),
        ] {
            assert_eq!(
                auto_levels(&rgb, lo, hi).unwrap_err().code,
                ErrorCode::InvalidInput,
                "{lo} {hi}"
            );
        }
        let nan = ImageF32::new(2, 2, 3, vec![f32::NAN; 12]).unwrap();
        assert_eq!(
            auto_levels(&nan, 0.5, 99.5).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    proptest! {
        /// More transmittance (less density) never gives a brighter positive.
        #[test]
        fn the_output_is_monotonic_in_density(
            a in uniform3(1e-7f32..2.0),
            b in uniform3(1e-7f32..2.0),
            black in 0.0f32..3.0,
            extra in 0.01f32..4.0,
        ) {
            let mut low = ImageF32::new(1, 1, 3, a.to_vec()).unwrap();
            let mut high = ImageF32::new(1, 1, 3, b.to_vec()).unwrap();
            let l = levels(black, black + extra);
            invert(&mut low, &l).unwrap();
            invert(&mut high, &l).unwrap();
            for c in 0..3 {
                if a[c] <= b[c] {
                    prop_assert!(low.data[c] >= high.data[c], "T {} <= {} but P {} < {}", a[c], b[c], low.data[c], high.data[c]);
                }
            }
        }

        #[test]
        fn the_output_is_finite_for_any_input(
            data in proptest::collection::vec(proptest::num::f32::ANY, 3..30),
        ) {
            let mut data = data;
            data.truncate(data.len() / 3 * 3);
            let pixels = data.len() / 3;
            let mut image = ImageF32::new(pixels as u32, 1, 3, data).unwrap();
            invert(&mut image, &levels(0.1, 2.5)).unwrap();
            prop_assert!(image.data.iter().all(|v| v.is_finite() && *v >= 0.0));
        }
    }

    /// A 4x2 image: the left half is `left`, the right half `right`.
    fn halves(left: [f32; 3], right: [f32; 3]) -> ImageF32 {
        let mut data = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                data.extend_from_slice(if x < 2 { &left } else { &right });
            }
        }
        ImageF32::new(4, 2, 3, data).unwrap()
    }

    const LEFT: Rect = Rect {
        x: 0,
        y: 0,
        w: 2,
        h: 2,
    };

    #[test]
    fn a_picked_white_sets_the_density_of_its_median() {
        let image = halves([0.1, 0.01, 0.001], [1.0; 3]);
        let set = levels_from_sample(&image, LEFT, LevelPoint::White, &levels(0.2, 1.0)).unwrap();
        for (got, want) in set.white.iter().zip([1.0, 2.0, 3.0]) {
            assert!((got - want).abs() < 1e-4, "{:?}", set.white);
        }
        assert_eq!(set.black, [0.2; 3]);
    }

    #[test]
    fn a_picked_black_sets_the_black_and_keeps_the_white() {
        let image = halves([0.5, 0.5, 0.5], [1.0; 3]);
        let set = levels_from_sample(&image, LEFT, LevelPoint::Black, &levels(0.0, 2.0)).unwrap();
        assert!((set.black[0] - std::f32::consts::LOG10_2).abs() < 1e-4);
        assert_eq!(set.white, [2.0; 3]);
    }

    #[test]
    fn white_stays_above_black_whichever_end_is_set() {
        let image = halves([0.5; 3], [1.0; 3]);
        // The area is denser than the white asked for: black gives way.
        let black = levels_from_sample(&image, LEFT, LevelPoint::Black, &levels(0.0, 0.1)).unwrap();
        assert!((0..3).all(|c| black.white[c] - black.black[c] >= INVERT_GAP * 0.99));
        // The area is thinner than the black: white gives way.
        let white = levels_from_sample(&image, LEFT, LevelPoint::White, &levels(1.0, 2.0)).unwrap();
        assert!((0..3).all(|c| white.white[c] - white.black[c] >= INVERT_GAP * 0.99));
    }

    #[test]
    fn a_median_ignores_a_few_stray_pixels() {
        let mut image = halves([0.1; 3], [1.0; 3]);
        image.data[0..3].copy_from_slice(&[1e-5; 3]);
        let set = levels_from_sample(&image, LEFT, LevelPoint::White, &levels(0.0, 5.0)).unwrap();
        assert!((set.white[0] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn empty_black_non_finite_and_outside_samples_are_rejected() {
        let current = levels(0.0, 1.0);
        let black = halves([0.0; 3], [1.0; 3]);
        let nan = halves([f32::NAN; 3], [1.0; 3]);
        let one_dark_channel = halves([0.5, 0.0, 0.5], [1.0; 3]);
        let ok = halves([0.5; 3], [1.0; 3]);
        for (image, area) in [
            (&black, LEFT),
            (&nan, LEFT),
            (&one_dark_channel, LEFT),
            (
                &ok,
                Rect {
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 2,
                },
            ),
            (
                &ok,
                Rect {
                    x: 3,
                    y: 0,
                    w: 2,
                    h: 2,
                },
            ),
        ] {
            let error = levels_from_sample(image, area, LevelPoint::White, &current).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidInput, "{area:?}");
        }
        let mono = ImageF32::new(2, 1, 1, vec![0.5, 0.5]).unwrap();
        let area = Rect {
            x: 0,
            y: 0,
            w: 2,
            h: 1,
        };
        assert!(levels_from_sample(&mono, area, LevelPoint::White, &current).is_err());
    }

    // ---- combined inversion of a black-and-white frame (ADR 0017) ----

    /// A black-and-white negative: the wedge is the same picture in every channel, seen with a
    /// different density gain per channel (the per-channel `black` and `white`), with `noise`
    /// (relative, in the transmittance, so the same density noise in every channel).
    fn bw_scan(noise: f32) -> Synthetic {
        synthetic::negative(
            11,
            &Params {
                width: 320,
                height: 200,
                pattern: Pattern::Wedge,
                black: [0.10, 0.12, 0.15],
                white: [2.10, 2.30, 2.50],
                noise,
                ..Params::default()
            },
        )
        .unwrap()
    }

    /// A small deterministic generator for extra noise in one channel.
    struct Lcg(u64);

    impl Lcg {
        /// Uniform in [-1, 1].
        fn unit(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
        }
    }

    /// Multiplies every sample of `channel` by `1 + amplitude * u`.
    fn add_noise(image: &mut ImageF32, channel: usize, amplitude: f32) {
        let mut rng = Lcg(99);
        for pixel in image.data.as_chunks_mut::<3>().0 {
            pixel[channel] *= 1.0 + amplitude * rng.unit();
        }
    }

    /// The standard deviation of `channel` over `patch`, relative to the span of that channel
    /// over the whole `image` (its 99.5th minus its 0.5th percentile).
    fn noise_to_span(image: &ImageF32, channel: usize, patch: Rect) -> f32 {
        let mut values = Vec::new();
        for y in patch.y..patch.y + patch.h {
            for x in patch.x..patch.x + patch.w {
                values.push(
                    image.data[(y as usize * image.width as usize + x as usize) * 3 + channel],
                );
            }
        }
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        let sd =
            (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32).sqrt();
        let mut all: Vec<f32> = image
            .data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p[channel])
            .collect();
        let span = percentile(&mut all, DEFAULT_HIGH_PERCENTILE)
            - percentile(&mut all, DEFAULT_LOW_PERCENTILE);
        sd / span
    }

    /// A flat patch inside the fifth step of the wedge, in the cropped frame.
    fn flat_patch(frame: &ImageF32) -> Rect {
        let step = frame.width / 8;
        Rect {
            x: step * 4 + step / 4,
            y: frame.height / 4,
            w: step / 2,
            h: frame.height / 2,
        }
    }

    /// The cropped frame of `s` inverted per channel and combined, as `(per channel, combined)`.
    fn invert_both_ways(s: &Synthetic) -> (ImageF32, ImageF32, Invert) {
        let frame = crop_to_frame(s, &normalized(s));
        let colour = auto_levels(&frame, DEFAULT_LOW_PERCENTILE, DEFAULT_HIGH_PERCENTILE).unwrap();
        let combined =
            auto_levels_combined(&frame, DEFAULT_LOW_PERCENTILE, DEFAULT_HIGH_PERCENTILE).unwrap();
        let mut per_channel = frame.clone();
        invert(&mut per_channel, &colour).unwrap();
        let mut one_plane = frame;
        invert(&mut one_plane, &combined).unwrap();
        (per_channel, one_plane, combined)
    }

    #[test]
    fn without_combine_the_inversion_is_the_per_channel_formula_bit_for_bit() {
        let s = scan(Pattern::Blobs, 0.05);
        let frame = crop_to_frame(&s, &normalized(&s));
        let levels = Invert {
            black: [0.11, 0.13, 0.17],
            white: [2.0, 2.2, 2.4],
            combine: None,
        };
        let mut got = frame.clone();
        invert(&mut got, &levels).unwrap();
        for (i, (g, t)) in got.data.iter().zip(&frame.data).enumerate() {
            let c = i % 3;
            let want =
                ((density(*t) - levels.black[c]) / (levels.white[c] - levels.black[c])).max(0.0);
            assert_eq!(g.to_bits(), want.to_bits(), "sample {i}");
        }
    }

    #[test]
    fn a_combined_inversion_writes_one_grey_plane_from_the_weighted_density() {
        let mut image = ImageF32::new(2, 1, 3, vec![1.0, 1.0, 1.0, 0.1, 0.01, 0.001]).unwrap();
        let levels = Invert {
            black: [0.0, 0.2, 0.4],
            white: [2.0, 2.0, 2.0],
            combine: Some([0.5, 0.25, 0.25]),
        };
        invert(&mut image, &levels).unwrap();
        // Points: black is the mean 0.2, white the mean 2.0. The base is density 0, so P = 0.
        assert_eq!(&image.data[..3], [0.0; 3]);
        // D = 0.5 * 1 + 0.25 * 2 + 0.25 * 3 = 1.75; P = (1.75 - 0.2) / 1.8.
        let want = (1.75f32 - 0.2) / 1.8;
        for v in &image.data[3..] {
            assert!((v - want).abs() < 1e-5, "{v} vs {want}");
        }
    }

    #[test]
    fn bad_combine_weights_or_points_are_invalid_input() {
        let image = ImageF32::zeros(2, 2, 3).unwrap();
        for combine in [
            [f32::NAN, 1.0, 1.0],
            [-1.0, 2.0, 1.0],
            [0.0, 0.0, 0.0],
            [f32::INFINITY, 0.0, 0.0],
        ] {
            let bad = Invert {
                combine: Some(combine),
                ..levels(0.0, 2.0)
            };
            assert_eq!(
                invert(&mut image.clone(), &bad).unwrap_err().code,
                ErrorCode::InvalidInput,
                "{combine:?}"
            );
        }
        let flat = Invert {
            combine: Some([1.0, 1.0, 1.0]),
            ..levels(1.0, 1.0)
        };
        assert!(invert(&mut image.clone(), &flat).is_err());
    }

    #[test]
    fn the_combined_plane_is_less_noisy_than_any_one_channel() {
        let s = bw_scan(0.05);
        let (per_channel, combined, levels) = invert_both_ways(&s);
        let weights = levels.combine.unwrap();
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        let patch = flat_patch(&combined);
        let own: Vec<f32> = (0..3)
            .map(|c| noise_to_span(&per_channel, c, patch))
            .collect();
        let joint = noise_to_span(&combined, 0, patch);
        for (c, noise) in own.iter().enumerate() {
            assert!(
                joint < *noise,
                "combined {joint} is not below channel {c} with {noise} (weights {weights:?})"
            );
        }
        // With equal noise the gain is real, not a rounding: about sqrt(sum of squared spans).
        assert!(
            joint < 0.85 * own.iter().copied().fold(f32::MAX, f32::min),
            "{joint} vs {own:?}"
        );
    }

    #[test]
    fn the_combined_plane_is_the_same_grey_in_every_channel() {
        let s = bw_scan(0.05);
        let (_, combined, _) = invert_both_ways(&s);
        for p in combined.data.as_chunks::<3>().0 {
            assert!(p[0] == p[1] && p[1] == p[2], "{p:?}");
        }
    }

    #[test]
    fn with_one_noisy_channel_the_combined_plane_is_no_worse_than_the_best_channel() {
        let mut s = bw_scan(0.03);
        add_noise(&mut s.image, 0, 0.30);
        let (per_channel, combined, levels) = invert_both_ways(&s);
        let weights = levels.combine.unwrap();
        assert!(
            weights[0] < weights[1] && weights[0] < weights[2],
            "the noisy channel counts least: {weights:?}"
        );
        let patch = flat_patch(&combined);
        let own: Vec<f32> = (0..3)
            .map(|c| noise_to_span(&per_channel, c, patch))
            .collect();
        let best = own.iter().copied().fold(f32::MAX, f32::min);
        let joint = noise_to_span(&combined, 0, patch);
        assert!(
            joint <= best * 1.05,
            "combined {joint} vs best {best} ({own:?})"
        );
        assert!(joint > 0.0);
    }

    #[test]
    fn the_weights_are_equal_for_a_noiseless_or_degenerate_image() {
        let equal = [1.0 / 3.0; 3];
        let clean = bw_scan(0.0);
        let frame = crop_to_frame(&clean, &normalized(&clean));
        assert_eq!(combine_weights(&frame).unwrap(), equal);
        let flat = ImageF32::new(8, 8, 3, vec![0.5; 192]).unwrap();
        assert_eq!(combine_weights(&flat).unwrap(), equal);
        let one_column = ImageF32::new(1, 6, 3, vec![0.5; 18]).unwrap();
        assert_eq!(combine_weights(&one_column).unwrap(), equal);
        let nan = ImageF32::new(2, 2, 3, vec![f32::NAN; 12]).unwrap();
        assert_eq!(combine_weights(&nan).unwrap(), equal);
        let gray = ImageF32::zeros(4, 4, 1).unwrap();
        assert_eq!(
            combine_weights(&gray).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn the_combined_auto_levels_are_one_pair_from_the_combined_percentiles() {
        let s = bw_scan(0.04);
        let frame = crop_to_frame(&s, &normalized(&s));
        let auto = auto_levels_combined(&frame, 0.5, 99.5).unwrap();
        assert!(auto.black[0] == auto.black[1] && auto.black[1] == auto.black[2]);
        assert!(auto.white[0] == auto.white[1] && auto.white[1] == auto.white[2]);
        let weights = auto.combine.unwrap();
        let total: f32 = weights.iter().sum();
        let mut all: Vec<f32> = frame
            .data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| combined_density(p.map(density), &weights, total))
            .collect();
        // The grid is every pixel here (the frame is far below the sample limit).
        assert_eq!(auto.black[0], percentile(&mut all, 0.5));
        assert_eq!(auto.white[0], percentile(&mut all, 99.5));
        assert!(auto.white[0] >= auto.black[0] + INVERT_GAP);
        assert_eq!(
            Recipe {
                invert: auto,
                ..Recipe::default()
            }
            .clamped()
            .invert,
            auto,
            "already inside the recipe's ranges"
        );
    }

    #[test]
    fn the_combined_auto_levels_stay_inside_the_ranges_for_odd_images() {
        for data in [vec![0.5; 192], vec![0.0; 192], vec![1.0e9; 192]] {
            let image = ImageF32::new(8, 8, 3, data).unwrap();
            let auto = auto_levels_combined(&image, 0.5, 99.5).unwrap();
            assert!(auto.white[0] >= auto.black[0] + INVERT_GAP);
            assert!(auto.white[0] <= INVERT_MAX && auto.black[0] >= 0.0);
        }
        let nan = ImageF32::new(2, 2, 3, vec![f32::NAN; 12]).unwrap();
        assert_eq!(
            auto_levels_combined(&nan, 0.5, 99.5).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        assert!(auto_levels_combined(&nan, 60.0, 40.0).is_err());
    }

    fn combined(black: f32, white: f32) -> Invert {
        Invert {
            combine: Some([1.0 / 3.0; 3]),
            ..levels(black, white)
        }
    }

    #[test]
    fn a_picked_point_on_a_combined_frame_is_one_value_in_all_three_channels() {
        // Densities 1, 2 and 3 with equal weights: the combined density is 2.
        let image = halves([0.1, 0.01, 0.001], [1.0; 3]);
        let white =
            levels_from_sample(&image, LEFT, LevelPoint::White, &combined(0.2, 5.0)).unwrap();
        for c in 0..3 {
            assert!((white.white[c] - 2.0).abs() < 1e-4, "{:?}", white.white);
        }
        assert_eq!(white.black, [0.2; 3]);
        assert_eq!(white.combine, Some([1.0 / 3.0; 3]));

        let black =
            levels_from_sample(&image, LEFT, LevelPoint::Black, &combined(0.0, 5.0)).unwrap();
        assert!((black.black[0] - 2.0).abs() < 1e-4);
        assert!(black.black[0] == black.black[1] && black.black[1] == black.black[2]);
        assert_eq!(black.white, [5.0; 3]);
    }

    #[test]
    fn a_picked_point_on_a_combined_frame_keeps_white_above_black() {
        let image = halves([0.5; 3], [1.0; 3]);
        let black =
            levels_from_sample(&image, LEFT, LevelPoint::Black, &combined(0.0, 0.1)).unwrap();
        assert!(black.white[0] - black.black[0] >= INVERT_GAP * 0.99);
        let white =
            levels_from_sample(&image, LEFT, LevelPoint::White, &combined(1.0, 2.0)).unwrap();
        assert!(white.white[0] - white.black[0] >= INVERT_GAP * 0.99);
        // Unequal points are brought to their means, which is what the inversion used.
        let unequal = Invert {
            black: [0.0, 0.3, 0.6],
            white: [2.0; 3],
            combine: Some([1.0 / 3.0; 3]),
        };
        let set = levels_from_sample(&image, LEFT, LevelPoint::White, &unequal).unwrap();
        assert_eq!(set.black, [0.3; 3]);
    }

    #[test]
    fn a_picked_point_on_a_combined_frame_refuses_black_and_empty_areas() {
        let current = combined(0.0, 1.0);
        let black = halves([0.0; 3], [1.0; 3]);
        let nan = halves([f32::NAN; 3], [1.0; 3]);
        let one_dark_channel = halves([0.5, 0.0, 0.5], [1.0; 3]);
        for image in [&black, &nan, &one_dark_channel] {
            let error = levels_from_sample(image, LEFT, LevelPoint::White, &current).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidInput);
        }
        let outside = Rect {
            x: 3,
            y: 0,
            w: 2,
            h: 2,
        };
        assert!(levels_from_sample(
            &halves([0.5; 3], [1.0; 3]),
            outside,
            LevelPoint::White,
            &current
        )
        .is_err());
    }

    proptest! {
        #[test]
        fn picked_levels_are_in_range_and_ordered(
            t in proptest::array::uniform3(1e-7f32..4.0),
            black in 0.0f32..9.9, white in 0.0f32..10.0, set_white: bool,
        ) {
            let image = halves(t, t);
            let area = Rect { x: 0, y: 0, w: 4, h: 2 };
            let point = if set_white { LevelPoint::White } else { LevelPoint::Black };
            let current = Invert { black: [black; 3], white: [white.max(black + INVERT_GAP); 3], combine: None };
            let set = levels_from_sample(&image, area, point, &current).unwrap();
            for c in 0..3 {
                prop_assert!(set.black[c] >= 0.0 && set.white[c] <= INVERT_MAX);
                prop_assert!(set.white[c] - set.black[c] >= INVERT_GAP * 0.99);
            }
        }
    }
}
