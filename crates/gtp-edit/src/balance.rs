//! Automatic colour balance (issue P-22): estimates the colour axes that make a converted frame's
//! neutral tones neutral, from the frame itself.
//!
//! The estimators look at the **inverted** frame (linear, before the colour stage) and find the
//! per-channel level `log2` of what should be grey. [`balance_color`] turns that into the three
//! colour axes with the same arithmetic as [`crate::neutral_balance`], so the sliders show the
//! correction and the user can move them.
//!
//! - [`BalanceMethod::GreyWorld`]: the geometric mean of each channel over the mid-tones
//!   ([`BALANCE_LUMA_LOW`] to [`BALANCE_LUMA_HIGH`] of the luma). Assumes the scene averages to grey.
//! - [`BalanceMethod::NearNeutral`]: starts from the grey-world estimate, then keeps only the
//!   pixels whose colour is within [`NEAR_NEUTRAL_RADIUS`] stops of it, and repeats
//!   [`NEAR_NEUTRAL_ROUNDS`] times, so large coloured areas (foliage, a sky) pull the estimate less.
//!
//! One correction for a whole roll: [`roll_balance`], the median over the frames of their axes.
//!
//! A frame that cannot be judged gets no correction (`None`): too few usable pixels, too few near
//! neutral ones, or a cast beyond [`MAX_BALANCE_AXIS`] of the slider range.

use gtp_core::recipe::Color;
use gtp_core::ImageF32;

use crate::color::axes_from_logs;
use crate::{luma, MAX_SAMPLE};

/// How the automatic colour balance is estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BalanceMethod {
    /// No correction (the colour axes stay at 0).
    #[default]
    Off,
    /// Mid-tone grey-world.
    GreyWorld,
    /// Grey-world restricted to the near-neutral pixels.
    NearNeutral,
}

/// Lowest luma, in linear light, that counts as a mid-tone.
pub const BALANCE_LUMA_LOW: f32 = 0.045;
/// Highest luma, in linear light, that counts as a mid-tone.
pub const BALANCE_LUMA_HIGH: f32 = 0.54;
/// Colour distance, in stops (the length of the vector of per-channel log2 differences), a pixel may
/// have from the current estimate and still count as near neutral.
pub const NEAR_NEUTRAL_RADIUS: f32 = 0.5;
/// How many times the near-neutral selection is repeated.
pub const NEAR_NEUTRAL_ROUNDS: usize = 3;
/// The fewest mid-tone pixels an estimate needs.
pub const MIN_BALANCE_PIXELS: usize = 500;
/// The smallest share of the mid-tone pixels that must be near neutral for that estimate.
pub const MIN_NEUTRAL_FRACTION: f32 = 0.05;
/// An estimate that needs an axis beyond this share of the slider range is not trusted.
pub const MAX_BALANCE_AXIS: f32 = 0.6;
/// The estimators look at about this many pixels at most, on a regular grid.
const MAX_BALANCE_SAMPLES: usize = 250_000;

/// The colour axes for `method` on an inverted frame, with everything else of `current` kept;
/// `None` when the method is off or the frame cannot be judged (see the module docs).
pub fn balance_color(inverted: &ImageF32, method: BalanceMethod, current: &Color) -> Option<Color> {
    let logs = estimate_neutral(inverted, method)?;
    let color = axes_from_logs(logs, current).ok()?;
    let limit = gtp_core::recipe::SLIDER_MAX * MAX_BALANCE_AXIS;
    (color.red_cyan.abs() <= limit
        && color.green_magenta.abs() <= limit
        && color.blue_yellow.abs() <= limit)
        .then_some(color)
}

/// One correction for a roll: the median of each colour axis over the frames that could be judged
/// (`[red_cyan, green_magenta, blue_yellow]` each, as [`balance_color`] gives them), or `None`
/// without any frame. The middle value ignores a frame whose picture is mostly one colour; with an
/// even count it is the mean of the two middle values. A frame with a value that is not finite is
/// not counted.
pub fn roll_balance(frames: &[[f32; 3]]) -> Option<[f32; 3]> {
    let usable: Vec<&[f32; 3]> = frames
        .iter()
        .filter(|f| f.iter().all(|v| v.is_finite()))
        .collect();
    if usable.is_empty() {
        return None;
    }
    let axis = |c: usize| {
        let mut values: Vec<f32> = usable.iter().map(|f| f[c]).collect();
        values.sort_by(f32::total_cmp);
        let middle = values.len() / 2;
        if values.len() % 2 == 1 {
            values[middle]
        } else {
            (values[middle - 1] + values[middle]) / 2.0
        }
    };
    Some([axis(0), axis(1), axis(2)])
}

/// The `log2` of the three channels of what the frame's neutral tones are, or `None`.
pub fn estimate_neutral(inverted: &ImageF32, method: BalanceMethod) -> Option<[f32; 3]> {
    if method == BalanceMethod::Off || inverted.channels != 3 {
        return None;
    }
    let pixels = mid_tones(inverted);
    if pixels.len() < MIN_BALANCE_PIXELS {
        return None;
    }
    let mut estimate = mean_logs(pixels.iter());
    if method == BalanceMethod::NearNeutral {
        for _ in 0..NEAR_NEUTRAL_ROUNDS {
            let kept: Vec<&[f32; 3]> = pixels
                .iter()
                .filter(|p| distance(p, &estimate) <= NEAR_NEUTRAL_RADIUS)
                .collect();
            if (kept.len() as f32) < MIN_NEUTRAL_FRACTION * pixels.len() as f32 {
                return None;
            }
            estimate = mean_logs(kept.into_iter());
        }
    }
    Some(estimate)
}

/// The `log2` of the channels of the sampled mid-tone pixels.
fn mid_tones(image: &ImageF32) -> Vec<[f32; 3]> {
    let total = image.data.len() / 3;
    let step = total.div_ceil(MAX_BALANCE_SAMPLES).max(1);
    image
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .step_by(step)
        .filter_map(|p| {
            let rgb = [p[0], p[1], p[2]];
            if rgb
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.0 || *v > MAX_SAMPLE)
            {
                return None;
            }
            let y = luma(rgb);
            (BALANCE_LUMA_LOW..=BALANCE_LUMA_HIGH)
                .contains(&y)
                .then(|| rgb.map(f32::log2))
        })
        .collect()
}

fn mean_logs<'a>(logs: impl Iterator<Item = &'a [f32; 3]>) -> [f32; 3] {
    let mut sum = [0.0f64; 3];
    let mut count = 0usize;
    for l in logs {
        for c in 0..3 {
            sum[c] += f64::from(l[c]);
        }
        count += 1;
    }
    let n = count.max(1) as f64;
    sum.map(|s| (s / n) as f32)
}

/// Distance in stops between the colours of two log pixels, ignoring their brightness.
fn distance(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let mean = (d[0] + d[1] + d[2]) / 3.0;
    d.iter()
        .map(|v| (v - mean) * (v - mean))
        .sum::<f32>()
        .sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame of mid-tone greys with the given cast (gains per channel).
    fn grey_frame(gains: [f32; 3]) -> ImageF32 {
        let mut data = Vec::new();
        for i in 0..4000 {
            let v = 0.1 + 0.2 * (i % 40) as f32 / 40.0;
            data.extend(gains.map(|g| v * g));
        }
        ImageF32 {
            width: 80,
            height: 50,
            channels: 3,
            data,
        }
    }

    #[test]
    fn a_neutral_frame_needs_no_correction() {
        let frame = grey_frame([1.0; 3]);
        for method in [BalanceMethod::GreyWorld, BalanceMethod::NearNeutral] {
            let color = balance_color(&frame, method, &Color::default()).unwrap();
            assert!(color.red_cyan.abs() < 0.01, "{method:?} {color:?}");
            assert!(color.green_magenta.abs() < 0.01);
            assert!(color.blue_yellow.abs() < 0.01);
        }
    }

    #[test]
    fn a_cast_is_corrected_towards_the_missing_colour() {
        // Too green and blue, so red must be added and green and blue lowered.
        let frame = grey_frame([0.8, 1.0, 1.1]);
        for method in [BalanceMethod::GreyWorld, BalanceMethod::NearNeutral] {
            let color = balance_color(&frame, method, &Color::default()).unwrap();
            assert!(color.red_cyan > 1.0, "{method:?} {color:?}");
            assert!(color.blue_yellow < 0.0);
        }
    }

    #[test]
    fn off_and_unjudgeable_frames_give_nothing() {
        let frame = grey_frame([1.0; 3]);
        assert!(balance_color(&frame, BalanceMethod::Off, &Color::default()).is_none());
        let dark = ImageF32 {
            width: 80,
            height: 50,
            channels: 3,
            data: vec![0.001; 80 * 50 * 3],
        };
        assert!(balance_color(&dark, BalanceMethod::GreyWorld, &Color::default()).is_none());
        let heavy = grey_frame([0.2, 1.0, 1.0]);
        assert!(balance_color(&heavy, BalanceMethod::GreyWorld, &Color::default()).is_none());
    }

    #[test]
    fn near_neutral_ignores_a_large_coloured_area() {
        // A third of the pixels are strongly green, the rest grey with a small cast.
        let mut frame = grey_frame([0.9, 1.0, 1.0]);
        for p in frame.data.as_chunks_mut::<3>().0.iter_mut().skip(2700) {
            p[1] *= 3.0;
        }
        let world = estimate_neutral(&frame, BalanceMethod::GreyWorld).unwrap();
        let near = estimate_neutral(&frame, BalanceMethod::NearNeutral).unwrap();
        let cast = |l: [f32; 3]| (l[1] - l[0]).abs();
        assert!(cast(near) < cast(world));
    }

    #[test]
    fn the_roll_median_takes_the_middle_of_each_axis() {
        assert_eq!(roll_balance(&[]), None);
        let odd = [[3.0, 0.0, 1.0], [1.0, 5.0, 1.0], [2.0, 1.0, 9.0]];
        assert_eq!(roll_balance(&odd), Some([2.0, 1.0, 1.0]));
        let even = [[1.0, 0.0, 4.0], [3.0, 2.0, 0.0]];
        assert_eq!(roll_balance(&even), Some([2.0, 1.0, 2.0]));
    }

    #[test]
    fn the_roll_median_ignores_one_odd_frame_and_a_single_frame_is_its_own_median() {
        let frames = [[1.0, 1.0, 1.0], [1.2, 0.9, 1.1], [30.0, -30.0, 30.0]];
        assert_eq!(roll_balance(&frames), Some([1.2, 0.9, 1.1]));
        assert_eq!(roll_balance(&frames[..1]), Some([1.0, 1.0, 1.0]));
    }

    #[test]
    fn the_roll_median_skips_values_that_are_not_numbers() {
        let frames = [[f32::NAN, 0.0, 0.0], [2.0, 4.0, 6.0]];
        assert_eq!(roll_balance(&frames), Some([2.0, 4.0, 6.0]));
        assert_eq!(roll_balance(&frames[..1]), None);
    }
}
