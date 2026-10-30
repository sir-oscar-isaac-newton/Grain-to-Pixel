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

use gtp_core::recipe::{Invert, INVERT_GAP, INVERT_MAX};
use gtp_core::{AppError, ErrorCode, ImageF32};
use rayon::prelude::*;

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

/// The value at `percent` (0 to 100) of the sorted `values`; reorders them. Not for empty slices.
fn percentile(values: &mut [f32], percent: f32) -> f32 {
    let index = ((percent / 100.0) * (values.len() - 1) as f32).round() as usize;
    let (_, value, _) = values.select_nth_unstable_by(index.min(values.len() - 1), f32::total_cmp);
    *value
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
    let ordered = low_percent.is_finite()
        && high_percent.is_finite()
        && 0.0 <= low_percent
        && low_percent < high_percent
        && high_percent <= 100.0;
    if !ordered {
        return Err(invalid(
            "The percentiles must satisfy 0 <= low < high <= 100.",
            format!("{low_percent}, {high_percent}"),
        ));
    }

    let pixels = u64::from(image.width) * u64::from(image.height);
    let step = ((pixels as f64 / MAX_LEVEL_SAMPLES as f64).sqrt().ceil() as usize).max(1);
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
}
