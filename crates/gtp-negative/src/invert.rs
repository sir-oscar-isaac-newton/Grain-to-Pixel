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

    proptest! {
        #[test]
        fn picked_levels_are_in_range_and_ordered(
            t in proptest::array::uniform3(1e-7f32..4.0),
            black in 0.0f32..9.9, white in 0.0f32..10.0, set_white: bool,
        ) {
            let image = halves(t, t);
            let area = Rect { x: 0, y: 0, w: 4, h: 2 };
            let point = if set_white { LevelPoint::White } else { LevelPoint::Black };
            let current = Invert { black: [black; 3], white: [white.max(black + INVERT_GAP); 3] };
            let set = levels_from_sample(&image, area, point, &current).unwrap();
            for c in 0..3 {
                prop_assert!(set.black[c] >= 0.0 && set.white[c] <= INVERT_MAX);
                prop_assert!(set.white[c] - set.black[c] >= INVERT_GAP * 0.99);
            }
        }
    }
}
