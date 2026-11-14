//! Colour mixer stage (docs/specification.md section 4.2, after step 5, the colour stage): a change
//! of hue, saturation and lightness for each of eight hue bands, in Oklch (ADR 0012).
//!
//! The image is linear Rec.2020. Per pixel the colour is converted to Oklab (see [`crate::oklab`])
//! and read as lightness `L`, chroma `C` and hue `h` in degrees.
//!
//! **Bands.** The eight bands sit at the hues [`MIXER_BAND_HUES`], in the order red, orange,
//! yellow, green, aqua, blue, purple, magenta (the recipe's index 0 to 7). For a hue `h` between
//! two neighbouring centres `c_i <= h < c_(i+1)` (cyclic), `x = (h - c_i) / (c_(i+1) - c_i)`; band
//! `i + 1` weighs `smoothstep(x)` and band `i` weighs `1 - smoothstep(x)`, every other band 0. The
//! weights add up to 1 everywhere, a band weighs 1 at its own centre, and the weights are
//! continuous with a continuous first derivative.
//!
//! **Gate.** `g = smoothstep(clamp(C / MIXER_CHROMA_KNEE, 0, 1))`: every change fades to nothing
//! towards grey, so noise in a neutral area and the hue of a grey are never amplified.
//!
//! **Changes.** With `H`, `S`, `Lt` the weighted sums of the recipe values divided by 100:
//! - hue: `h' = h + H * MIXER_HUE_MAX_DEG * g`;
//! - saturation: `C' = max(0, C * (1 + S * g))`, so -100 removes the colour and +100 doubles it;
//! - lightness: a gain of `2^(Lt * MIXER_LIGHTNESS_MAX_EV * g)` in linear light, applied as
//!   `L' = L * gain^(1/3)` and `C'' = C' * gain^(1/3)`. Scaling `L`, `a` and `b` by the cube root of
//!   a gain scales the linear RGB by the gain exactly, so hue and saturation are kept.
//!
//! The result is converted back and not clamped. A pixel is left untouched, bit for bit, when `g`
//! is 0 or the three weighted sums are all 0; non-finite pixels are left as they are. A mixer of 24
//! zeros returns the image untouched.

use gtp_core::recipe::{Mixer, MIXER_BANDS};
use gtp_core::{AppError, ImageF32};
use rayon::prelude::*;

use crate::oklab::{hue_deg, oklab_to_rgb, rgb_to_oklab, FULL_TURN_DEG};
use crate::require_rgb;

/// The Oklch hue, in degrees, at the centre of each band: red, orange, yellow, green, aqua, blue,
/// purple, magenta. Red, yellow, green, aqua, blue and magenta are the hues of the sRGB primaries
/// and secondaries (converted to linear Rec.2020 first), rounded to whole degrees; orange is
/// midway between red and yellow, purple midway between blue and magenta. A test derives them
/// again.
pub const MIXER_BAND_HUES: [f32; MIXER_BANDS] =
    [29.0, 70.0, 110.0, 143.0, 195.0, 264.0, 296.0, 328.0];
/// Chroma at which the gate is fully open; below it every change fades out.
pub const MIXER_CHROMA_KNEE: f32 = 0.04;
/// Degrees of hue shift at a hue value of +-100 (and a gate of 1).
pub const MIXER_HUE_MAX_DEG: f32 = 30.0;
/// Stops of linear light at a lightness value of +-100 (and a gate of 1).
pub const MIXER_LIGHTNESS_MAX_EV: f32 = 1.0;

/// Full scale of the recipe values.
const PERCENT: f32 = 100.0;
/// Exponent that turns a gain in linear light into the factor on Oklab's `L`, `a` and `b`.
const GAIN_TO_LAB_EXPONENT: f32 = 1.0 / 3.0;

fn smoothstep(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-PERCENT, PERCENT)
    } else {
        0.0
    }
}

/// The two bands that weigh at a hue: the lower one, the next one (cyclic) and the weight of the
/// next one; the lower one weighs 1 minus that.
fn locate(hue: f32) -> (usize, usize, f32) {
    let first = MIXER_BAND_HUES[0];
    let h = if hue < first {
        hue + FULL_TURN_DEG
    } else {
        hue
    };
    let mut lower = MIXER_BANDS - 1;
    for (i, centre) in MIXER_BAND_HUES.iter().enumerate() {
        if *centre <= h {
            lower = i;
        }
    }
    let upper = (lower + 1) % MIXER_BANDS;
    let mut end = MIXER_BAND_HUES[upper];
    if upper == 0 {
        end += FULL_TURN_DEG;
    }
    let x = ((h - MIXER_BAND_HUES[lower]) / (end - MIXER_BAND_HUES[lower])).clamp(0.0, 1.0);
    (lower, upper, smoothstep(x))
}

/// Applies the colour mixer in place. The recipe is expected to be clamped; the values are limited
/// here as well, so no input can produce NaN.
pub fn apply_mixer(image: &mut ImageF32, mixer: &Mixer) -> Result<(), AppError> {
    require_rgb(image, "colour mixer")?;
    let limit = |values: &[f32; MIXER_BANDS]| values.map(|v| finite_or_zero(v) / PERCENT);
    let (hue, saturation, lightness) = (
        limit(&mixer.hue),
        limit(&mixer.saturation),
        limit(&mixer.lightness),
    );
    let all_zero = hue
        .iter()
        .chain(&saturation)
        .chain(&lightness)
        .all(|v| *v == 0.0);
    if all_zero || image.data.is_empty() {
        return Ok(());
    }
    let row_len = image.width as usize * 3;
    image.data.par_chunks_mut(row_len).for_each(|row| {
        let (pixels, _) = row.as_chunks_mut::<3>();
        for pixel in pixels {
            if !pixel.iter().all(|c| c.is_finite()) {
                continue;
            }
            let [l, a, b] = rgb_to_oklab(*pixel);
            let chroma = a.hypot(b);
            let gate = smoothstep((chroma / MIXER_CHROMA_KNEE).clamp(0.0, 1.0));
            if gate == 0.0 {
                continue;
            }
            let h = hue_deg(a, b);
            let (lower, upper, weight_upper) = locate(h);
            let weight_lower = 1.0 - weight_upper;
            let mix = |v: &[f32; MIXER_BANDS]| weight_lower * v[lower] + weight_upper * v[upper];
            let (dh, ds, dl) = (mix(&hue), mix(&saturation), mix(&lightness));
            if dh == 0.0 && ds == 0.0 && dl == 0.0 {
                continue;
            }
            let new_hue = h + dh * MIXER_HUE_MAX_DEG * gate;
            let new_chroma = (chroma * (1.0 + ds * gate)).max(0.0);
            let scale = (dl * MIXER_LIGHTNESS_MAX_EV * gate * GAIN_TO_LAB_EXPONENT).exp2();
            let (sin, cos) = new_hue.to_radians().sin_cos();
            let (new_chroma, new_l) = (new_chroma * scale, l * scale);
            let out = oklab_to_rgb([new_l, new_chroma * cos, new_chroma * sin]);
            if out.iter().all(|c| c.is_finite()) {
                *pixel = out;
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oklab::{oklch_to_rgb, rgb_to_oklch};

    /// Linear sRGB to linear Rec.2020 (ITU-R BT.2087).
    const SRGB_TO_REC2020: [[f32; 3]; 3] = [
        [0.627_404, 0.329_282, 0.043_313_6],
        [0.069_097, 0.919_54, 0.011_361_2],
        [0.016_391_6, 0.088_013_2, 0.895_595],
    ];

    fn stream(seed: u32) -> impl FnMut() -> f32 {
        let mut state = seed;
        move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32
        }
    }

    fn flat(width: u32, height: u32, rgb: [f32; 3]) -> ImageF32 {
        let data = (0..width * height).flat_map(|_| rgb).collect();
        ImageF32::new(width, height, 3, data).unwrap()
    }

    /// A mixer with one band set on each of the three lists.
    fn band(index: usize, hue: f32, saturation: f32, lightness: f32) -> Mixer {
        let mut m = Mixer::default();
        m.hue[index] = hue;
        m.saturation[index] = saturation;
        m.lightness[index] = lightness;
        m
    }

    fn mixed(rgb: [f32; 3], mixer: &Mixer) -> [f32; 3] {
        let mut image = flat(1, 1, rgb);
        apply_mixer(&mut image, mixer).unwrap();
        [image.data[0], image.data[1], image.data[2]]
    }

    fn lch(l: f32, c: f32, h: f32) -> [f32; 3] {
        oklch_to_rgb([l, c, h])
    }

    fn hue_distance(a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(FULL_TURN_DEG);
        d.min(FULL_TURN_DEG - d)
    }

    #[test]
    fn all_zeros_leave_the_image_bit_for_bit() {
        let mut image = flat(37, 23, [0.1, 0.5, 2.5]);
        image.data[10] = 0.123_456_79;
        image.data[11] = f32::NAN;
        let before = image.clone();
        apply_mixer(&mut image, &Mixer::default()).unwrap();
        assert_eq!(
            image.data.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            before.data.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_pixel_with_no_active_change_is_bit_for_bit_unchanged_when_other_bands_are_set() {
        // Only red is set; aqua, blue and a neutral are far from it.
        let mixer = band(0, 100.0, -100.0, 100.0);
        for rgb in [lch(0.6, 0.1, 195.0), lch(0.6, 0.1, 264.0), [0.3, 0.3, 0.3]] {
            assert_eq!(mixed(rgb, &mixer), rgb);
        }
        // At orange's centre only orange and its neighbours weigh; green's value changes nothing.
        let rgb = lch(0.7, 0.1, MIXER_BAND_HUES[1]);
        assert_eq!(mixed(rgb, &band(3, 100.0, 100.0, 100.0)), rgb);
        assert_ne!(mixed(rgb, &band(1, 100.0, 0.0, 0.0)), rgb);
    }

    #[test]
    fn the_band_hues_are_increasing_and_derived_from_the_primaries() {
        assert!(MIXER_BAND_HUES.windows(2).all(|w| w[0] < w[1]));
        const { assert!(MIXER_BAND_HUES[0] > 0.0 && MIXER_BAND_HUES[7] < FULL_TURN_DEG) };
        let hue_of = |srgb: [f32; 3]| {
            let m = SRGB_TO_REC2020;
            let rgb = [0, 1, 2].map(|i| m[i][0] * srgb[0] + m[i][1] * srgb[1] + m[i][2] * srgb[2]);
            rgb_to_oklch(rgb)[2]
        };
        let red = hue_of([1.0, 0.0, 0.0]);
        let yellow = hue_of([1.0, 1.0, 0.0]);
        let green = hue_of([0.0, 1.0, 0.0]);
        let aqua = hue_of([0.0, 1.0, 1.0]);
        let blue = hue_of([0.0, 0.0, 1.0]);
        let magenta = hue_of([1.0, 0.0, 1.0]);
        let derived = [
            red,
            (red + yellow) / 2.0,
            yellow,
            green,
            aqua,
            blue,
            (blue + magenta) / 2.0,
            magenta,
        ];
        for (constant, derived) in MIXER_BAND_HUES.iter().zip(derived) {
            assert!((constant - derived).abs() <= 1.0, "{constant} vs {derived}");
        }
    }

    #[test]
    fn the_weights_add_up_to_one_and_a_centre_weighs_one() {
        for step in 0..3600 {
            let h = step as f32 * 0.1;
            let (lower, upper, w) = locate(h);
            assert_eq!(upper, (lower + 1) % MIXER_BANDS);
            assert!((0.0..=1.0).contains(&w), "{h}: {w}");
            assert!(((1.0 - w) + w - 1.0).abs() < 1e-6);
        }
        for (i, centre) in MIXER_BAND_HUES.iter().enumerate() {
            let (lower, upper, w) = locate(*centre);
            assert_eq!((lower, w), (i, 0.0), "centre {i}");
            assert_eq!(upper, (i + 1) % MIXER_BANDS);
            // Just before the next centre the next band weighs 1.
            let next = MIXER_BAND_HUES[(i + 1) % MIXER_BANDS];
            let (lower, _, w) = locate(next - 1e-3);
            assert_eq!(lower, i);
            assert!(w > 0.999_99, "{w}");
        }
        // The wrap: hues below the first centre belong to the last band and the first.
        assert_eq!(locate(5.0).0, MIXER_BANDS - 1);
        assert_eq!(locate(359.0).0, MIXER_BANDS - 1);
    }

    #[test]
    fn each_band_moves_its_own_colour_and_leaves_the_opposite_one_alone() {
        for (i, centre) in MIXER_BAND_HUES.iter().copied().enumerate() {
            let (l, c) = (0.6, 0.1);
            let rgb = lch(l, c, centre);
            let opposite = lch(l, c, centre + 180.0);

            for (value, sign) in [(100.0, 1.0), (-100.0, -1.0)] {
                let out = rgb_to_oklch(mixed(rgb, &band(i, value, 0.0, 0.0)));
                let want = (centre + sign * MIXER_HUE_MAX_DEG).rem_euclid(FULL_TURN_DEG);
                assert!(hue_distance(out[2], want) < 0.1, "band {i}: {out:?} {want}");
                assert!((out[0] - l).abs() < 1e-3 && (out[1] - c).abs() < 1e-3);
            }

            let grey = rgb_to_oklch(mixed(rgb, &band(i, 0.0, -100.0, 0.0)));
            assert!(grey[1] < 1e-4, "band {i}: {grey:?}");
            let more = rgb_to_oklch(mixed(rgb, &band(i, 0.0, 100.0, 0.0)));
            assert!((more[1] - 2.0 * c).abs() < 1e-3, "band {i}: {more:?}");
            assert!(hue_distance(more[2], centre) < 0.05 && (more[0] - l).abs() < 1e-3);

            for (value, gain) in [(100.0, 2.0), (-100.0, 0.5)] {
                let out = mixed(rgb, &band(i, 0.0, 0.0, value));
                for k in 0..3 {
                    let want = rgb[k] * gain;
                    assert!(
                        (out[k] - want).abs() < 1e-3 * (1.0 + want.abs()),
                        "band {i}"
                    );
                }
                let lch_out = rgb_to_oklch(out);
                assert!(hue_distance(lch_out[2], centre) < 0.05);
                assert!((lch_out[1] / lch_out[0] - c / l).abs() < 1e-3);
            }

            for mixer in [
                band(i, 100.0, 100.0, 100.0),
                band(i, -100.0, -100.0, -100.0),
            ] {
                assert_eq!(mixed(opposite, &mixer), opposite, "band {i}");
            }
        }
    }

    #[test]
    fn a_hue_sweep_has_no_jump_at_any_setting() {
        let mut next = stream(5);
        let mut settings = vec![
            band(0, 100.0, 100.0, 100.0),
            band(7, -100.0, -100.0, -100.0),
        ];
        let alternating = |a: f32, b: f32| {
            let pick = |i: usize| if i.is_multiple_of(2) { a } else { b };
            Mixer {
                hue: [0, 1, 2, 3, 4, 5, 6, 7].map(pick),
                saturation: [0, 1, 2, 3, 4, 5, 6, 7].map(|i| pick(i + 1)),
                lightness: [0, 1, 2, 3, 4, 5, 6, 7].map(pick),
            }
        };
        settings.push(alternating(100.0, -100.0));
        settings.push(alternating(-100.0, 100.0));
        for _ in 0..3 {
            let mut random = || [(); 8].map(|_| next() * 200.0 - 100.0);
            settings.push(Mixer {
                hue: random(),
                saturation: random(),
                lightness: random(),
            });
        }
        for mixer in settings {
            let mut previous: Option<[f32; 3]> = None;
            let mut biggest = 0.0_f32;
            for step in 0..=720 {
                let h = step as f32 * 0.5;
                let out = rgb_to_oklab(mixed(lch(0.6, 0.1, h), &mixer));
                if let Some(p) = previous {
                    let d: [f32; 3] = [0, 1, 2].map(|k| out[k] - p[k]);
                    biggest = biggest.max((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt());
                }
                previous = Some(out);
            }
            // A step of half a degree at a chroma of 0.1 is 0.0009 in Oklab; the steepest settings
            // stay within a few times that, a jump of a band value would be far above.
            assert!(biggest < 0.02, "{mixer:?}: {biggest}");
        }
    }

    #[test]
    fn greys_are_unchanged_and_near_greys_change_smoothly() {
        let loud = Mixer {
            hue: [100.0; 8],
            saturation: [100.0; 8],
            lightness: [100.0; 8],
        };
        for v in [0.0, 1e-5, 0.18, 0.5, 1.0, 4.0] {
            let out = mixed([v; 3], &loud);
            for c in out {
                assert!((c - v).abs() <= 1e-4 * v.max(1e-3), "{v}: {out:?}");
            }
        }
        let mut previous: Option<[f32; 3]> = None;
        for step in 0..=400 {
            let chroma = step as f32 * 1e-4;
            let rgb = lch(0.5, chroma, 120.0);
            let out = rgb_to_oklab(mixed(rgb, &loud));
            if let Some(p) = previous {
                let d: [f32; 3] = [0, 1, 2].map(|k| out[k] - p[k]);
                assert!(
                    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() < 2e-3,
                    "{step}"
                );
            }
            previous = Some(out);
        }
        let faint = lch(0.5, 1e-4, 120.0);
        let out = mixed(faint, &loud);
        for k in 0..3 {
            assert!((out[k] - faint[k]).abs() < 1e-4, "{faint:?} {out:?}");
        }
    }

    #[test]
    fn nothing_becomes_non_finite_and_bad_input_stays_out() {
        let loud = Mixer {
            hue: [100.0; 8],
            saturation: [-100.0; 8],
            lightness: [100.0; 8],
        };
        let mut next = stream(9);
        for _ in 0..2000 {
            let rgb = [next() * 4.0 - 0.5, next() * 4.0 - 0.5, next() * 4.0 - 0.5];
            assert!(mixed(rgb, &loud).iter().all(|v| v.is_finite()), "{rgb:?}");
        }
        let nan = Mixer {
            hue: [f32::NAN; 8],
            saturation: [f32::INFINITY; 8],
            lightness: [900.0; 8],
        };
        let out = mixed(lch(0.6, 0.1, 40.0), &nan);
        assert!(out.iter().all(|v| v.is_finite()));
        let rgb = [f32::NAN, 0.5, 0.2];
        let out = mixed(rgb, &loud);
        assert!(out[0].is_nan() && out[1] == 0.5 && out[2] == 0.2);
        let out = mixed([f32::INFINITY, 0.5, 0.2], &loud);
        assert!(out[0].is_infinite() && out[1] == 0.5);
        let hot = mixed(lch(0.9, 0.2, 29.0), &band(0, 0.0, 0.0, 100.0));
        assert!(hot.iter().any(|v| *v > 1.0), "values above 1 are kept");
    }

    #[test]
    fn a_gray_image_is_an_error() {
        let mut gray = ImageF32::zeros(4, 4, 1).unwrap();
        assert!(apply_mixer(&mut gray, &band(0, 10.0, 0.0, 0.0)).is_err());
    }

    #[test]
    fn the_result_does_not_depend_on_the_image_size() {
        let mixer = band(0, 70.0, 60.0, -20.0);
        let rgb = lch(0.6, 0.1, 40.0);
        let (mut small, mut large) = (flat(5, 3, rgb), flat(200, 120, rgb));
        apply_mixer(&mut small, &mixer).unwrap();
        apply_mixer(&mut large, &mixer).unwrap();
        assert_eq!(small.data[..3], large.data[..3]);
        assert_eq!(small.data[..3], mixed(rgb, &mixer));
    }

    #[test]
    fn the_result_does_not_depend_on_the_thread_count() {
        let make = || {
            let mut next = stream(3);
            let data = (0..97 * 61 * 3).map(|_| next() * 1.5).collect();
            let mut image = ImageF32::new(97, 61, 3, data).unwrap();
            let mixer = Mixer {
                hue: [30.0, -20.0, 0.0, 80.0, -90.0, 10.0, 0.0, 55.0],
                saturation: [-40.0, 60.0, 20.0, 0.0, 100.0, -100.0, 5.0, 0.0],
                lightness: [10.0, 0.0, -30.0, 70.0, 0.0, 25.0, -60.0, 15.0],
            };
            apply_mixer(&mut image, &mixer).unwrap();
            image
        };
        let one = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(make);
        assert_eq!(one, make());
    }

    /// Cost of the stage on a 6000 x 4000 image; run with
    /// `cargo test -p gtp-edit --release -- --ignored mixer_timing --nocapture`.
    #[test]
    #[ignore = "timing only"]
    fn mixer_timing_on_a_6000_by_4000_image() {
        let mut next = stream(1);
        let data = (0..6000 * 4000 * 3).map(|_| next()).collect();
        let mut image = ImageF32::new(6000, 4000, 3, data).unwrap();
        let mixer = Mixer {
            hue: [20.0, 0.0, -30.0, 0.0, 40.0, 0.0, 0.0, 10.0],
            saturation: [0.0, 30.0, 0.0, -50.0, 0.0, 20.0, 0.0, 0.0],
            lightness: [0.0, 0.0, 15.0, 0.0, 0.0, -25.0, 0.0, 30.0],
        };
        let start = std::time::Instant::now();
        apply_mixer(&mut image, &mixer).unwrap();
        println!("apply_mixer on 6000x4000: {:?}", start.elapsed());
        assert!(image.data.iter().all(|v| v.is_finite()));
    }

    /// Where the time of the mixer goes on a 6000 x 4000 image of random colours (so nearly every
    /// pixel has an open gate, the worst case): the stage is split into phases that each run over one
    /// row at a time (the row stays in cache) with the CPU time of each phase summed over the
    /// threads, and checked to give the same pixels as `apply_mixer`. The phases are: A the
    /// conversion to Oklab, B chroma and gate (`hypot`), C the hue (`atan2`), D the band weights
    /// (`locate` and the three weighted sums), E the new hue, chroma and lightness (`sin_cos`,
    /// `exp2`), F the conversion back. CPU ms divided by the threads is about the wall time. Run with
    /// `cargo test --release -p gtp-edit mixer_breakdown -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing only"]
    fn mixer_breakdown_on_a_6000_by_4000_image() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::Instant;

        let mut next = stream(1);
        let data = (0..6000 * 4000 * 3).map(|_| next()).collect();
        let image = ImageF32::new(6000, 4000, 3, data).unwrap();
        let mixer = Mixer {
            hue: [20.0, 5.0, -30.0, 8.0, 40.0, -6.0, 9.0, 10.0],
            saturation: [10.0, 30.0, 4.0, -50.0, 7.0, 20.0, -9.0, 5.0],
            lightness: [6.0, 8.0, 15.0, -4.0, 9.0, -25.0, 3.0, 30.0],
        };
        let limit = |values: &[f32; MIXER_BANDS]| values.map(|v| finite_or_zero(v) / PERCENT);
        let (hue, saturation, lightness) = (
            limit(&mixer.hue),
            limit(&mixer.saturation),
            limit(&mixer.lightness),
        );
        let width = image.width as usize;

        let mut reference = image.clone();
        apply_mixer(&mut reference, &mixer).unwrap();

        for threads in [1_usize, 6, 12] {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            let mut whole_runs = Vec::new();
            let mut phase_runs: Vec<[f64; 6]> = Vec::new();
            let mut active_share = 0.0;
            pool.install(|| {
                for run in 0..=5 {
                    let mut copy = image.clone();
                    let start = Instant::now();
                    apply_mixer(&mut copy, &mixer).unwrap();
                    let whole = start.elapsed().as_secs_f64() * 1000.0;

                    let mut split = image.clone();
                    let nanos: [AtomicU64; 6] = Default::default();
                    let active_total = AtomicU64::new(0);
                    let wall = Instant::now();
                    split.data.par_chunks_mut(width * 3).for_each_init(
                        || {
                            (
                                vec![[0.0_f32; 3]; width],
                                vec![0.0_f32; width],
                                vec![0.0_f32; width],
                                vec![0.0_f32; width],
                                vec![false; width],
                                vec![[0.0_f32; 3]; width],
                            )
                        },
                        |(lab, chroma, gate, hues, active, shifts), row| {
                            let (pixels, _) = row.as_chunks_mut::<3>();
                            let mut mark = Instant::now();
                            let lap = |slot: usize, mark: &mut Instant| {
                                let now = Instant::now();
                                nanos[slot]
                                    .fetch_add((now - *mark).as_nanos() as u64, Ordering::Relaxed);
                                *mark = now;
                            };
                            // A
                            for (i, pixel) in pixels.iter().enumerate() {
                                active[i] = pixel.iter().all(|c| c.is_finite());
                                if active[i] {
                                    lab[i] = rgb_to_oklab(*pixel);
                                }
                            }
                            lap(0, &mut mark);
                            // B
                            for i in 0..pixels.len() {
                                if active[i] {
                                    chroma[i] = lab[i][1].hypot(lab[i][2]);
                                    gate[i] =
                                        smoothstep((chroma[i] / MIXER_CHROMA_KNEE).clamp(0.0, 1.0));
                                    active[i] = gate[i] != 0.0;
                                }
                            }
                            lap(1, &mut mark);
                            // C
                            for i in 0..pixels.len() {
                                if active[i] {
                                    hues[i] = hue_deg(lab[i][1], lab[i][2]);
                                }
                            }
                            lap(2, &mut mark);
                            // D
                            for i in 0..pixels.len() {
                                if active[i] {
                                    let (lower, upper, weight_upper) = locate(hues[i]);
                                    let weight_lower = 1.0 - weight_upper;
                                    let mix = |v: &[f32; MIXER_BANDS]| {
                                        weight_lower * v[lower] + weight_upper * v[upper]
                                    };
                                    shifts[i] = [mix(&hue), mix(&saturation), mix(&lightness)];
                                    active[i] = !(shifts[i] == [0.0; 3]);
                                }
                            }
                            lap(3, &mut mark);
                            // E
                            for i in 0..pixels.len() {
                                if active[i] {
                                    let [dh, ds, dl] = shifts[i];
                                    let new_hue = hues[i] + dh * MIXER_HUE_MAX_DEG * gate[i];
                                    let new_chroma = (chroma[i] * (1.0 + ds * gate[i])).max(0.0);
                                    let scale = (dl
                                        * MIXER_LIGHTNESS_MAX_EV
                                        * gate[i]
                                        * GAIN_TO_LAB_EXPONENT)
                                        .exp2();
                                    let (sin, cos) = new_hue.to_radians().sin_cos();
                                    let (new_chroma, new_l) =
                                        (new_chroma * scale, lab[i][0] * scale);
                                    lab[i] = [new_l, new_chroma * cos, new_chroma * sin];
                                }
                            }
                            lap(4, &mut mark);
                            // F
                            let mut count = 0;
                            for (i, pixel) in pixels.iter_mut().enumerate() {
                                if active[i] {
                                    count += 1;
                                    let out = oklab_to_rgb(lab[i]);
                                    if out.iter().all(|c| c.is_finite()) {
                                        *pixel = out;
                                    }
                                }
                            }
                            lap(5, &mut mark);
                            active_total.fetch_add(count, Ordering::Relaxed);
                        },
                    );
                    let split_wall = wall.elapsed().as_secs_f64() * 1000.0;
                    assert!(split == copy && copy == reference, "the phases agree");
                    if run > 0 {
                        whole_runs.push((whole, split_wall));
                        phase_runs.push(nanos.map(|n| n.load(Ordering::Relaxed) as f64 / 1e6));
                        active_share =
                            active_total.load(Ordering::Relaxed) as f64 / (6000.0 * 4000.0);
                    }
                }
            });
            let median = |mut v: Vec<f64>| {
                v.sort_by(f64::total_cmp);
                v[v.len() / 2]
            };
            eprintln!(
                "--- mixer, {threads} thread(s), median of 5: apply_mixer {:.1} ms wall, the phased copy {:.1} ms wall; {:.0}% of the pixels changed ---",
                median(whole_runs.iter().map(|r| r.0).collect()),
                median(whole_runs.iter().map(|r| r.1).collect()),
                active_share * 100.0
            );
            for (n, name) in [
                "A rgb_to_oklab",
                "B hypot + gate",
                "C hue (atan2)",
                "D band weights",
                "E new hue/chroma/L (sin_cos, exp2)",
                "F oklab_to_rgb + store",
            ]
            .iter()
            .enumerate()
            {
                eprintln!(
                    "  {name:<38} {:>8.1} ms CPU",
                    median(phase_runs.iter().map(|r| r[n]).collect())
                );
            }
        }
    }
}
