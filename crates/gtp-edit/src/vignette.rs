//! Vignette stage (docs/specification.md section 4.2, after step 8): a gain that depends on the
//! distance from the centre of the picture.
//!
//! The distance is normalised per axis so that every corner is at 1: with pixel centres at
//! `(x + 0.5, y + 0.5)`, `nx = (x + 0.5 - w/2) / (w/2)`, `ny` likewise, and
//! `d = sqrt((nx^2 + ny^2) / 2)`. The shape follows the picture's proportions and does not depend
//! on its size. The effect starts at `r0 = midpoint / 100` and grows to the corner as
//! `t = smoothstep((d - r0) / (1 - r0))`; at `r0 >= 1` there is no effect. The gain is
//! `2^(amount / 100 * VIGNETTE_MAX_EV * t)` on all three channels in linear light, so hue and
//! saturation do not change. The result is not clamped. An amount of 0 returns the image untouched.
//!
//! The gain depends on the pixel position only, and the offsets of pixel `i` and pixel `len - 1 - i`
//! along an axis are exactly opposite, so the gains of one quarter of the picture serve all four
//! (checked on the bits of the offsets before it is used; see `is_mirror_symmetric`).

use gtp_core::recipe::Vignette;
use gtp_core::{AppError, ImageF32};
use rayon::prelude::*;

use crate::require_rgb;

/// Stops of gain at the corners at an amount of +-100.
pub const VIGNETTE_MAX_EV: f32 = 2.0;

/// The falloff `t` (0 at and inside the start radius, 1 at the corners) at distance `d`.
fn falloff(d: f32, r0: f32) -> f32 {
    if r0 >= 1.0 {
        return 0.0;
    }
    let x = ((d - r0) / (1.0 - r0)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// The squared normalised coordinate of every pixel centre along an axis of `len` pixels:
/// `((i + 0.5 - len/2) / (len/2))^2`.
fn squared_offsets(len: usize) -> Vec<f32> {
    let half = len as f32 / 2.0;
    (0..len)
        .map(|i| {
            let n = (i as f32 + 0.5 - half) / half;
            n * n
        })
        .collect()
}

/// True when pixel `i` and pixel `len - 1 - i` have the same bits in `offsets`, for every `i`. They
/// do whenever `i + 0.5 - len/2` is exact in `f32` (every size below 2^23): the two numerators are
/// then exactly opposite, and division and squaring are symmetric in the sign. The check is made
/// anyway, so that a size where it does not hold simply takes the direct path.
fn is_mirror_symmetric(offsets: &[f32]) -> bool {
    offsets
        .iter()
        .zip(offsets.iter().rev())
        .all(|(a, b)| a.to_bits() == b.to_bits())
}

/// The gain at squared offsets `nx2` and `ny2` (0.0 where there is no effect; a real gain is never
/// 0, it is at least 2^-2).
fn gain_at(nx2: f32, ny2: f32, r0: f32, stops: f32) -> f32 {
    let d = ((nx2 + ny2) / 2.0).sqrt();
    let t = falloff(d, r0);
    if t > 0.0 {
        (stops * t).exp2()
    } else {
        0.0
    }
}

/// Multiplies the pixels of `row` by their gains (a gain of 0.0 leaves a pixel alone).
fn scale_row(row: &mut [f32], gains: &[f32]) {
    let (pixels, _) = row.as_chunks_mut::<3>();
    for (pixel, gain) in pixels.iter_mut().zip(gains) {
        if *gain != 0.0 {
            for c in pixel {
                *c *= *gain;
            }
        }
    }
}

/// Applies the vignette in place. The recipe is expected to be clamped; the values are limited here
/// as well, so no input can produce NaN.
pub fn apply_vignette(image: &mut ImageF32, vignette: &Vignette) -> Result<(), AppError> {
    apply_vignette_with(image, vignette, true)
}

/// [`apply_vignette`]; `mirror` allows computing the gains of one quarter of the picture and
/// copying them to the other three (when their bits are the same, see [`is_mirror_symmetric`]).
fn apply_vignette_with(
    image: &mut ImageF32,
    vignette: &Vignette,
    mirror: bool,
) -> Result<(), AppError> {
    require_rgb(image, "vignette")?;
    let amount = if vignette.amount.is_finite() {
        vignette.amount.clamp(-100.0, 100.0)
    } else {
        0.0
    };
    if amount == 0.0 || image.width == 0 || image.height == 0 {
        return Ok(());
    }
    let midpoint = if vignette.midpoint.is_finite() {
        vignette.midpoint.clamp(0.0, 100.0)
    } else {
        Vignette::default().midpoint
    };
    let r0 = midpoint / 100.0;
    let stops = amount / 100.0 * VIGNETTE_MAX_EV;
    let (width, height) = (image.width as usize, image.height as usize);
    let (across, down) = (squared_offsets(width), squared_offsets(height));
    let mirror_x = mirror && is_mirror_symmetric(&across);
    let mirror_y = mirror && is_mirror_symmetric(&down);
    // Columns whose gain is computed; the others are copies of their mirror image.
    let computed = if mirror_x { width.div_ceil(2) } else { width };
    let row_len = width * 3;
    let gains_of_row = |gains: &mut Vec<f32>, y: usize| {
        gains.resize(width, 0.0);
        for x in 0..computed {
            gains[x] = gain_at(across[x], down[y], r0, stops);
        }
        for x in computed..width {
            gains[x] = gains[width - 1 - x];
        }
    };

    if mirror_y {
        // Row `y` of the top half and row `height - 1 - y` have the same gains: one set of gains
        // serves both. With an odd height the middle row is its own mirror image.
        let half = height / 2;
        let (top, rest) = image.data.split_at_mut(half * row_len);
        let (middle, bottom) = rest.split_at_mut((height - 2 * half) * row_len);
        top.par_chunks_mut(row_len)
            .zip(bottom.par_chunks_mut(row_len).rev())
            .enumerate()
            .for_each_init(Vec::new, |gains, (y, (upper, lower))| {
                gains_of_row(gains, y);
                scale_row(upper, gains);
                scale_row(lower, gains);
            });
        if !middle.is_empty() {
            let mut gains = Vec::new();
            gains_of_row(&mut gains, half);
            scale_row(middle, &gains);
        }
    } else {
        image
            .data
            .par_chunks_mut(row_len)
            .enumerate()
            .for_each_init(Vec::new, |gains, (y, row)| {
                gains_of_row(gains, y);
                scale_row(row, gains);
            });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: u32, height: u32, rgb: [f32; 3]) -> ImageF32 {
        let data = (0..width * height).flat_map(|_| rgb).collect();
        ImageF32::new(width, height, 3, data).unwrap()
    }

    fn vig(amount: f32, midpoint: f32) -> Vignette {
        Vignette { amount, midpoint }
    }

    fn px(image: &ImageF32, x: u32, y: u32) -> [f32; 3] {
        let i = (y * image.width + x) as usize * 3;
        [image.data[i], image.data[i + 1], image.data[i + 2]]
    }

    #[test]
    fn amount_zero_leaves_the_image_bit_for_bit() {
        let mut image = flat(37, 23, [0.1, 0.5, 2.5]);
        image.data[10] = 0.123_456_79;
        let before = image.clone();
        for midpoint in [0.0, 50.0, 100.0] {
            apply_vignette(&mut image, &vig(0.0, midpoint)).unwrap();
            assert_eq!(image, before);
        }
    }

    #[test]
    fn the_centre_is_unchanged_for_any_amount_and_midpoint_below_100() {
        // Odd sizes have a pixel centre exactly at the centre of the picture.
        for amount in [-100.0, -40.0, 30.0, 100.0] {
            for midpoint in [0.0, 25.0, 50.0, 99.0] {
                let mut image = flat(101, 101, [0.2, 0.4, 0.6]);
                apply_vignette(&mut image, &vig(amount, midpoint)).unwrap();
                let c = px(&image, 50, 50);
                for (got, want) in c.iter().zip([0.2, 0.4, 0.6]) {
                    assert!((got - want).abs() < 1e-6, "{amount} {midpoint}: {got}");
                }
            }
        }
    }

    #[test]
    fn the_corners_move_by_the_maximum_number_of_stops() {
        for (amount, factor) in [(-100.0_f32, 0.25_f32), (100.0, 4.0)] {
            let mut image = flat(400, 300, [0.5, 0.5, 0.5]);
            apply_vignette(&mut image, &vig(amount, 50.0)).unwrap();
            // The outermost pixel centres sit a hair inside the corner, so allow a little.
            let corner = px(&image, 0, 0)[0];
            let want = 0.5 * factor;
            assert!((corner / want - 1.0).abs() < 0.01, "{corner} vs {want}");
        }
        assert_eq!(VIGNETTE_MAX_EV, 2.0);
        let mut dark = flat(64, 64, [0.5; 3]);
        apply_vignette(&mut dark, &vig(-60.0, 50.0)).unwrap();
        assert!(px(&dark, 0, 0)[0] < 0.5);
        let mut light = flat(64, 64, [0.5; 3]);
        apply_vignette(&mut light, &vig(60.0, 50.0)).unwrap();
        assert!(px(&light, 0, 0)[0] > 0.5);
    }

    #[test]
    fn hue_and_saturation_are_kept() {
        let rgb = [0.1, 0.3, 0.7];
        let mut image = flat(80, 60, rgb);
        apply_vignette(&mut image, &vig(-80.0, 30.0)).unwrap();
        for (x, y) in [(0, 0), (79, 59), (10, 30), (40, 30), (70, 5)] {
            let p = px(&image, x, y);
            assert!((p[0] / p[1] - rgb[0] / rgb[1]).abs() < 1e-5);
            assert!((p[2] / p[1] - rgb[2] / rgb[1]).abs() < 1e-5);
        }
    }

    #[test]
    fn the_effect_grows_from_the_centre_to_the_corner() {
        for (amount, midpoint) in [(-100.0, 0.0), (-100.0, 50.0), (100.0, 20.0), (100.0, 80.0)] {
            let mut image = flat(201, 201, [0.5; 3]);
            apply_vignette(&mut image, &vig(amount, midpoint)).unwrap();
            let mut last = px(&image, 100, 100)[0];
            for step in 1..=100 {
                let v = px(&image, 100 + step, 100 + step)[0];
                if amount < 0.0 {
                    assert!(v <= last + 1e-7, "{amount} {midpoint} step {step}");
                } else {
                    assert!(v >= last - 1e-7, "{amount} {midpoint} step {step}");
                }
                last = v;
            }
        }
    }

    #[test]
    fn the_falloff_is_the_same_at_every_image_size() {
        let (mut small, mut large) = (flat(200, 100, [0.5; 3]), flat(800, 400, [0.5; 3]));
        apply_vignette(&mut small, &vig(-70.0, 30.0)).unwrap();
        apply_vignette(&mut large, &vig(-70.0, 30.0)).unwrap();
        for (fx, fy) in [
            (0.1, 0.2),
            (0.25, 0.5),
            (0.5, 0.9),
            (0.75, 0.25),
            (0.9, 0.5),
        ] {
            let a = px(&small, (fx * 200.0) as u32, (fy * 100.0) as u32)[0];
            let b = px(&large, (fx * 800.0) as u32, (fy * 400.0) as u32)[0];
            assert!((a - b).abs() < 0.02, "({fx}, {fy}): {a} vs {b}");
        }
    }

    #[test]
    fn midpoint_zero_starts_at_the_centre_and_100_has_no_effect() {
        let mut image = flat(101, 101, [0.5; 3]);
        apply_vignette(&mut image, &vig(-100.0, 0.0)).unwrap();
        assert!(
            px(&image, 60, 50)[0] < 0.5,
            "already dark close to the centre"
        );
        let mut image = flat(101, 101, [0.5; 3]);
        let before = image.clone();
        apply_vignette(&mut image, &vig(-100.0, 100.0)).unwrap();
        assert_eq!(image, before, "the degenerate start radius changes nothing");
    }

    #[test]
    fn nothing_is_not_a_number_and_the_result_is_not_clamped() {
        for (amount, midpoint) in [
            (-100.0, 0.0),
            (100.0, 0.0),
            (100.0, 100.0),
            (f32::NAN, 50.0),
            (50.0, f32::NAN),
            (f32::INFINITY, 50.0),
        ] {
            let mut image = flat(33, 17, [0.9, 0.5, 0.0]);
            apply_vignette(&mut image, &vig(amount, midpoint)).unwrap();
            assert!(
                image.data.iter().all(|v| v.is_finite()),
                "{amount} {midpoint}"
            );
        }
        let mut bright = flat(40, 40, [0.9; 3]);
        apply_vignette(&mut bright, &vig(100.0, 0.0)).unwrap();
        assert!(px(&bright, 0, 0)[0] > 1.0, "values above 1 are kept");
    }

    #[test]
    fn a_gray_image_is_an_error() {
        let mut gray = ImageF32::zeros(4, 4, 1).unwrap();
        assert!(apply_vignette(&mut gray, &vig(10.0, 50.0)).is_err());
    }

    #[test]
    fn the_result_does_not_depend_on_the_thread_count() {
        let make = || {
            let mut image = flat(97, 61, [0.3, 0.6, 0.9]);
            apply_vignette(&mut image, &vig(-55.0, 35.0)).unwrap();
            image
        };
        let one = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(make);
        assert_eq!(one, make());
    }

    /// `apply_vignette` before P-31: the gain computed directly for every pixel.
    fn apply_vignette_before_p31(
        image: &mut ImageF32,
        vignette: &Vignette,
    ) -> Result<(), AppError> {
        require_rgb(image, "vignette")?;
        let amount = if vignette.amount.is_finite() {
            vignette.amount.clamp(-100.0, 100.0)
        } else {
            0.0
        };
        if amount == 0.0 || image.width == 0 || image.height == 0 {
            return Ok(());
        }
        let midpoint = if vignette.midpoint.is_finite() {
            vignette.midpoint.clamp(0.0, 100.0)
        } else {
            Vignette::default().midpoint
        };
        let r0 = midpoint / 100.0;
        let stops = amount / 100.0 * VIGNETTE_MAX_EV;
        let (half_w, half_h) = (image.width as f32 / 2.0, image.height as f32 / 2.0);
        let row_len = image.width as usize * 3;
        image
            .data
            .par_chunks_mut(row_len)
            .enumerate()
            .for_each(|(y, row)| {
                let ny = (y as f32 + 0.5 - half_h) / half_h;
                let (pixels, _) = row.as_chunks_mut::<3>();
                for (x, pixel) in pixels.iter_mut().enumerate() {
                    let nx = (x as f32 + 0.5 - half_w) / half_w;
                    let d = ((nx * nx + ny * ny) / 2.0).sqrt();
                    let t = falloff(d, r0);
                    if t > 0.0 {
                        let gain = (stops * t).exp2();
                        for c in pixel {
                            *c *= gain;
                        }
                    }
                }
            });
        Ok(())
    }

    /// Values that are not all alike, with the odd negative, huge, zero and non-finite sample.
    fn varied(width: u32, height: u32, seed: u32) -> ImageF32 {
        let mut state = seed;
        let data = (0..width * height * 3)
            .map(|i| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let v = (state >> 8) as f32 / (1u32 << 24) as f32;
                match i % 29 {
                    3 => 0.0,
                    8 => -v,
                    13 => v * 1.0e7,
                    20 => f32::NAN,
                    24 => f32::INFINITY,
                    _ => v * 2.0,
                }
            })
            .collect();
        ImageF32::new(width, height, 3, data).unwrap()
    }

    #[test]
    fn the_vignette_has_the_same_bits_as_before_p31() {
        // Odd and even sizes, the smallest ones, wide and tall pictures.
        let sizes = [
            (1, 1),
            (1, 2),
            (2, 1),
            (2, 2),
            (3, 3),
            (7, 5),
            (8, 6),
            (5, 200),
            (200, 5),
            (64, 64),
            (101, 53),
            (100, 50),
            (257, 31),
            (130, 129),
        ];
        let settings = [
            (-100.0, 0.0),
            (-100.0, 50.0),
            (-33.3, 25.0),
            (0.7, 99.5),
            (50.0, 10.0),
            (100.0, 0.0),
            (100.0, 100.0),
            (80.0, f32::NAN),
        ];
        for (n, &(width, height)) in sizes.iter().enumerate() {
            let image = varied(width, height, 11 + n as u32);
            for (amount, midpoint) in settings {
                let mut old = image.clone();
                apply_vignette_before_p31(&mut old, &vig(amount, midpoint)).unwrap();
                for mirror in [true, false] {
                    let mut new = image.clone();
                    apply_vignette_with(&mut new, &vig(amount, midpoint), mirror).unwrap();
                    for (i, (n, o)) in new.data.iter().zip(&old.data).enumerate() {
                        assert_eq!(
                            n.to_bits(),
                            o.to_bits(),
                            "{width}x{height}, amount {amount}, midpoint {midpoint}, \
                             mirror {mirror}: sample {i}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_offsets_of_mirror_pixels_have_the_same_bits_up_to_the_size_limit() {
        // Every size of a small range, then the sizes around the largest the app accepts.
        for len in (1..300).chain([1000, 4999, 5000, 6000, 6001, 19_999, 20_000]) {
            assert!(is_mirror_symmetric(&squared_offsets(len)), "length {len}");
        }
    }
}
