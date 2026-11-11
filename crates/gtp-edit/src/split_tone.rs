//! Split toning stage (docs/specification.md section 4.2, after step 6, the tone stage): one tint
//! for the highlights and one for the shadows, blended in at the luma of each pixel, so it changes
//! colour and never brightness.
//!
//! The image is linear Rec.2020. Per pixel, with `y` the luma (a pixel with `y <= 0` is left as
//! it is), the tonal position is `t = clamp(y, 0, 1)^(1/SPLIT_TONE_GAMMA)` and the balance is
//! `b = balance / 100`. The highlight weight is `wh = smoothstep(clamp(t + b, 0, 1))` and the
//! shadow weight `ws = 1 - wh`, so a balance of +100 gives the highlight tint to every tone, -100
//! gives the shadow tint to every tone, and 0 crosses over at `t = 0.5`.
//!
//! A tint is the fully saturated colour of a hue (HSV with value and saturation 1, read as linear
//! Rec.2020), divided by its own luma so that its luma is 1. The blend amounts are
//! `ah = highlight_strength / 100 * SPLIT_TONE_MAX_BLEND * wh` and likewise `as` for the shadows,
//! and `out = pixel * (1 - ah - as) + y * th * ah + y * ts * as`. As both tints have luma 1, the
//! luma of the result is `y`. The result is not clamped. Both strengths at 0 return the image
//! untouched.

use gtp_core::recipe::SplitTone;
use gtp_core::{AppError, ImageF32};
use rayon::prelude::*;

use crate::{luma, require_rgb};

/// Gamma that turns a linear luma into the tonal position (`t = y^(1/gamma)`) that the balance
/// and the crossover work on, so the crossover sits at a perceptual middle grey.
pub const SPLIT_TONE_GAMMA: f32 = 2.2;
/// Share of the tint blended in at a strength of 100 and a weight of 1.
pub const SPLIT_TONE_MAX_BLEND: f32 = 0.5;

/// Degrees of hue in one of the six sectors of the colour wheel.
const HUE_SECTOR_DEG: f32 = 60.0;
/// Sectors on the colour wheel.
const HUE_SECTORS: f32 = 6.0;
/// Where each channel's hue ramp is offset on the wheel (red, green, blue), in sectors.
const HUE_OFFSETS: [f32; 3] = [5.0, 3.0, 1.0];
/// Where on the shifted wheel (in sectors) a channel is at its lowest: it is 0 there and 1 at a
/// distance of two sectors or more.
const HUE_RAMP_CENTRE: f32 = 2.0;
/// Sectors a ramp spends at its low end (the ramp is `|distance - centre| - this`, clamped).
const HUE_RAMP_FLOOR: f32 = 1.0;
/// Full scale of the percent values (strengths and balance).
const PERCENT: f32 = 100.0;
/// Full turn of the hue, in degrees.
const HUE_MAX_DEG: f32 = HUE_SECTORS * HUE_SECTOR_DEG;

/// The fully saturated colour of `hue_deg` (HSV rule, value 1, saturation 1).
fn hue_rgb(hue_deg: f32) -> [f32; 3] {
    let sector = hue_deg.rem_euclid(HUE_MAX_DEG) / HUE_SECTOR_DEG;
    HUE_OFFSETS.map(|offset| {
        let distance = ((sector + offset).rem_euclid(HUE_SECTORS) - HUE_RAMP_CENTRE).abs();
        (distance - HUE_RAMP_FLOOR).clamp(0.0, 1.0)
    })
}

/// The tint of a hue: its colour scaled to a luma of 1.
fn tint(hue_deg: f32) -> [f32; 3] {
    let rgb = hue_rgb(hue_deg);
    let y = luma(rgb);
    rgb.map(|c| c / y)
}

fn smoothstep(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

fn finite_or(value: f32, lo: f32, hi: f32, default: f32) -> f32 {
    if value.is_finite() {
        value.clamp(lo, hi)
    } else {
        default
    }
}

/// Applies the split toning in place. The recipe is expected to be clamped; the values are limited
/// here as well, so no input can produce NaN.
pub fn apply_split_tone(image: &mut ImageF32, split: &SplitTone) -> Result<(), AppError> {
    require_rgb(image, "split toning")?;
    let defaults = SplitTone::default();
    let highlight_strength = finite_or(
        split.highlight_strength,
        0.0,
        PERCENT,
        defaults.highlight_strength,
    );
    let shadow_strength = finite_or(
        split.shadow_strength,
        0.0,
        PERCENT,
        defaults.shadow_strength,
    );
    if (highlight_strength == 0.0 && shadow_strength == 0.0) || image.data.is_empty() {
        return Ok(());
    }
    let highlight_hue = finite_or(
        split.highlight_hue,
        0.0,
        HUE_MAX_DEG,
        defaults.highlight_hue,
    );
    let shadow_hue = finite_or(split.shadow_hue, 0.0, HUE_MAX_DEG, defaults.shadow_hue);
    let balance = finite_or(split.balance, -PERCENT, PERCENT, defaults.balance) / PERCENT;
    let (th, ts) = (tint(highlight_hue), tint(shadow_hue));
    let highlight_max = highlight_strength / PERCENT * SPLIT_TONE_MAX_BLEND;
    let shadow_max = shadow_strength / PERCENT * SPLIT_TONE_MAX_BLEND;
    let inverse_gamma = 1.0 / SPLIT_TONE_GAMMA;
    let row_len = image.width as usize * 3;
    image.data.par_chunks_mut(row_len).for_each(|row| {
        let (pixels, _) = row.as_chunks_mut::<3>();
        for pixel in pixels {
            let y = luma(*pixel);
            if !y.is_finite() || y <= 0.0 {
                continue;
            }
            let t = y.clamp(0.0, 1.0).powf(inverse_gamma);
            let wh = smoothstep((t + balance).clamp(0.0, 1.0));
            let ws = 1.0 - wh;
            let (ah, a_s) = (highlight_max * wh, shadow_max * ws);
            let keep = 1.0 - ah - a_s;
            for (c, (h, s)) in pixel.iter_mut().zip(th.into_iter().zip(ts)) {
                *c = *c * keep + y * h * ah + y * s * a_s;
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: u32, height: u32, rgb: [f32; 3]) -> ImageF32 {
        let data = (0..width * height).flat_map(|_| rgb).collect();
        ImageF32::new(width, height, 3, data).unwrap()
    }

    fn st(hh: f32, hs: f32, sh: f32, ss: f32, balance: f32) -> SplitTone {
        SplitTone {
            highlight_hue: hh,
            highlight_strength: hs,
            shadow_hue: sh,
            shadow_strength: ss,
            balance,
        }
    }

    fn toned(rgb: [f32; 3], split: &SplitTone) -> [f32; 3] {
        let mut image = flat(1, 1, rgb);
        apply_split_tone(&mut image, split).unwrap();
        [image.data[0], image.data[1], image.data[2]]
    }

    /// A deterministic stream of numbers in 0..1 (no dependency, no hardware).
    fn stream(seed: u32) -> impl FnMut() -> f32 {
        let mut state = seed;
        move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32
        }
    }

    #[test]
    fn both_strengths_zero_leave_the_image_bit_for_bit() {
        let mut image = flat(37, 23, [0.1, 0.5, 2.5]);
        image.data[10] = 0.123_456_79;
        let before = image.clone();
        for balance in [-100.0, 0.0, 70.0] {
            apply_split_tone(&mut image, &st(10.0, 0.0, 200.0, 0.0, balance)).unwrap();
            assert_eq!(image, before);
        }
    }

    #[test]
    fn the_hue_colours_follow_the_wheel() {
        assert_eq!(hue_rgb(0.0), [1.0, 0.0, 0.0]);
        assert_eq!(hue_rgb(360.0), [1.0, 0.0, 0.0]);
        assert_eq!(hue_rgb(120.0), [0.0, 1.0, 0.0]);
        assert_eq!(hue_rgb(240.0), [0.0, 0.0, 1.0]);
        let yellow = hue_rgb(60.0);
        assert!((yellow[0] - 1.0).abs() < 1e-6 && (yellow[1] - 1.0).abs() < 1e-6);
        assert!(yellow[2].abs() < 1e-6);
        let orange = hue_rgb(30.0);
        assert!((orange[1] - 0.5).abs() < 1e-6 && orange[2].abs() < 1e-6);
        for hue in [0.0, 17.0, 40.0, 95.0, 180.0, 220.0, 301.0, 360.0] {
            assert!((luma(tint(hue)) - 1.0).abs() < 1e-5, "{hue}");
        }
    }

    #[test]
    fn the_luma_is_kept_for_random_colours_at_any_setting() {
        let mut next = stream(7);
        for _ in 0..300 {
            let rgb = [next() * 3.0, next() * 3.0, next() * 3.0];
            let split = st(
                next() * 360.0,
                next() * 100.0,
                next() * 360.0,
                next() * 100.0,
                next() * 200.0 - 100.0,
            );
            let out = toned(rgb, &split);
            let (before, after) = (luma(rgb), luma(out));
            assert!(
                (after - before).abs() <= 1e-4 * before.max(1e-3),
                "{rgb:?} {split:?}: {before} vs {after}"
            );
        }
    }

    #[test]
    fn a_highlight_tint_moves_bright_pixels_and_barely_touches_dark_ones() {
        // Hue 0 is red: the red share of a grey pixel grows with the tint.
        let split = st(0.0, 100.0, 220.0, 0.0, 0.0);
        let bright = toned([0.8, 0.8, 0.8], &split);
        assert!(
            bright[0] > 0.8 && bright[1] < 0.8 && bright[2] < 0.8,
            "{bright:?}"
        );
        let dark = toned([0.002, 0.002, 0.002], &split);
        for c in dark {
            assert!((c - 0.002).abs() < 0.002 * 0.02, "{dark:?}");
        }
    }

    #[test]
    fn a_shadow_tint_moves_dark_pixels_and_barely_touches_bright_ones() {
        let split = st(40.0, 0.0, 240.0, 100.0, 0.0);
        let dark = toned([0.02, 0.02, 0.02], &split);
        assert!(
            dark[2] > 0.02 && dark[0] < 0.02 && dark[1] < 0.02,
            "{dark:?}"
        );
        let bright = toned([0.95, 0.95, 0.95], &split);
        for c in bright {
            assert!((c - 0.95).abs() < 0.95 * 0.03, "{bright:?}");
        }
    }

    #[test]
    fn the_balance_ends_give_one_tint_at_every_tone_and_zero_crosses_in_the_middle() {
        // Red highlights, blue shadows, both at full strength: the blend is at most 0.5.
        let split = |balance| st(0.0, 100.0, 240.0, 100.0, balance);
        let red = [1.0 / 0.2627, 0.0, 0.0];
        let blue = [0.0, 0.0, 1.0 / 0.0593];
        let blend = |g: f32, tint: [f32; 3]| tint.map(|c| g * 0.5 + g * c * 0.5);
        for g in [0.0005, 0.01, 0.18, 0.5, 0.9] {
            let up = toned([g; 3], &split(100.0));
            let down = toned([g; 3], &split(-100.0));
            let (want_up, want_down) = (blend(g, red), blend(g, blue));
            for i in 0..3 {
                assert!((up[i] - want_up[i]).abs() < 1e-5 * (1.0 + g * 20.0), "{g}");
                assert!(
                    (down[i] - want_down[i]).abs() < 1e-5 * (1.0 + g * 20.0),
                    "{g}"
                );
            }
        }
        // At balance 0 the crossover is at t = 0.5: both tints weigh the same.
        let g = 0.5_f32.powf(SPLIT_TONE_GAMMA);
        let mid = toned([g; 3], &split(0.0));
        for i in 0..3 {
            let want = g * 0.5 + g * (red[i] + blue[i]) * 0.25;
            assert!((mid[i] - want).abs() < 1e-5, "{mid:?} {want}");
        }
    }

    #[test]
    fn a_neutral_grey_takes_the_tint_in_the_order_of_the_hue() {
        let green = toned([0.4; 3], &st(120.0, 100.0, 120.0, 100.0, 0.0));
        assert!(
            green[1] > 0.4 && green[0] < 0.4 && green[2] < 0.4,
            "{green:?}"
        );
        let blue = toned([0.4; 3], &st(240.0, 100.0, 240.0, 100.0, 0.0));
        assert!(blue[2] > 0.4 && blue[0] < 0.4 && blue[1] < 0.4, "{blue:?}");
        let yellow = toned([0.4; 3], &st(60.0, 100.0, 60.0, 100.0, 0.0));
        assert!(
            yellow[0] > 0.4 && yellow[1] > 0.4 && yellow[2] < 0.4,
            "{yellow:?}"
        );
    }

    #[test]
    fn black_stays_black_and_non_finite_input_stays_out() {
        assert_eq!(
            toned([0.0; 3], &st(0.0, 100.0, 240.0, 100.0, 0.0)),
            [0.0; 3]
        );
        for (hh, hs, sh, ss, b) in [
            (f32::NAN, 50.0, 10.0, 50.0, 0.0),
            (10.0, f32::NAN, 10.0, 50.0, 0.0),
            (10.0, 50.0, f32::INFINITY, 50.0, f32::NAN),
            (10.0, 50.0, 10.0, f32::NAN, f32::NEG_INFINITY),
            (-50.0, 900.0, 900.0, 900.0, 900.0),
        ] {
            let out = toned([0.9, 0.5, 0.0], &st(hh, hs, sh, ss, b));
            assert!(out.iter().all(|v| v.is_finite()), "{out:?}");
        }
        let out = toned([f32::NAN, 0.5, 0.2], &st(0.0, 100.0, 240.0, 100.0, 0.0));
        assert!(
            out[1] == 0.5 && out[2] == 0.2,
            "a non-finite pixel is left as it is"
        );
        let hot = toned([4.0, 4.0, 4.0], &st(240.0, 100.0, 240.0, 100.0, 0.0));
        assert!(hot[2] > 4.0, "values above 1 are kept, not clamped");
    }

    #[test]
    fn the_result_does_not_depend_on_the_image_size() {
        let split = st(30.0, 70.0, 210.0, 60.0, -20.0);
        let rgb = [0.3, 0.2, 0.1];
        let (mut small, mut large) = (flat(5, 3, rgb), flat(200, 120, rgb));
        apply_split_tone(&mut small, &split).unwrap();
        apply_split_tone(&mut large, &split).unwrap();
        assert_eq!(small.data[..3], large.data[..3]);
        assert_eq!(small.data[..3], toned(rgb, &split));
    }

    #[test]
    fn a_gray_image_is_an_error() {
        let mut gray = ImageF32::zeros(4, 4, 1).unwrap();
        assert!(apply_split_tone(&mut gray, &st(0.0, 10.0, 0.0, 10.0, 0.0)).is_err());
    }

    #[test]
    fn the_result_does_not_depend_on_the_thread_count() {
        let make = || {
            let mut next = stream(3);
            let data = (0..97 * 61 * 3).map(|_| next() * 1.5).collect();
            let mut image = ImageF32::new(97, 61, 3, data).unwrap();
            apply_split_tone(&mut image, &st(35.0, 80.0, 215.0, 55.0, 25.0)).unwrap();
            image
        };
        let one = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(make);
        assert_eq!(one, make());
    }
}
