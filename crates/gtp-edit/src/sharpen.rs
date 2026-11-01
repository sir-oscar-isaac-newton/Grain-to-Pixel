//! Output sharpening (docs/specification.md section 4.2, step 9): an unsharp mask on the luminance
//! of the finished image. It is not part of the render, so previews and thumbnails are never
//! sharpened; the export applies it once, to the image at its final size.
//!
//! ```text
//! E  = Y' encoded for sharpening: Y^(1/GAMMA)  (Y is the Rec.2020 luma of the linear pixel)
//! B  = E blurred with a Gaussian of standard deviation radius_px, edges repeated
//! E2 = max(0, E + amount/100 * (E - B))
//! pixel = pixel * (E2^GAMMA / Y)
//! ```
//!
//! The blur and the difference are taken on the encoded luma, because that is what the eye sees
//! as an edge: in linear light the halo would be strong on the bright side and weak on the dark
//! one. Scaling the whole pixel by the new luma over the old keeps hue and saturation, so no
//! colour fringes appear. An `amount` of 0 changes nothing at all, and a flat image stays flat
//! (the kernel's weights sum to 1). The radius is in pixels of the image it is given and is not
//! scaled: a larger export wants a larger radius for the same look.
//!
//! Samples are limited to `[0, MAX_SAMPLE]` first (not finite becomes 0), as in the other stages.

use gtp_core::recipe::Sharpen;
use gtp_core::{AppError, ImageF32};
use rayon::prelude::*;

use crate::{luma, require_rgb, MAX_SAMPLE};

/// The power of the encoding in which edges are measured (close to the sRGB curve).
pub const SHARPEN_GAMMA: f32 = 2.4;
/// The kernel reaches this many standard deviations to each side.
pub const KERNEL_SIGMAS: f32 = 3.0;
/// Below this luma a pixel is black and is left alone (its scale would be a division by nothing).
const BLACK: f32 = 1e-9;

/// The Gaussian's weights for offsets `-r..=r`, summing to 1.
fn kernel(sigma: f32) -> Vec<f32> {
    let reach = (KERNEL_SIGMAS * sigma).ceil().max(1.0) as i64;
    let weights: Vec<f32> = (-reach..=reach)
        .map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = weights.iter().sum();
    weights.into_iter().map(|w| w / total).collect()
}

/// `plane` blurred along the rows, edges repeated.
fn blur_rows(plane: &[f32], width: usize, weights: &[f32]) -> Vec<f32> {
    let reach = weights.len() / 2;
    let mut out = vec![0.0f32; plane.len()];
    out.par_chunks_mut(width)
        .zip(plane.par_chunks(width))
        .for_each(|(dst, src)| {
            for (x, slot) in dst.iter_mut().enumerate() {
                let mut sum = 0.0f32;
                for (k, weight) in weights.iter().enumerate() {
                    let at = (x + k).saturating_sub(reach).min(width - 1);
                    sum += weight * src[at];
                }
                *slot = sum;
            }
        });
    out
}

/// `plane` blurred along the columns, edges repeated.
fn blur_columns(plane: &[f32], width: usize, height: usize, weights: &[f32]) -> Vec<f32> {
    let reach = weights.len() / 2;
    let mut out = vec![0.0f32; plane.len()];
    out.par_chunks_mut(width).enumerate().for_each(|(y, dst)| {
        for (k, weight) in weights.iter().enumerate() {
            let row = (y + k).saturating_sub(reach).min(height - 1);
            let src = &plane[row * width..(row + 1) * width];
            for (slot, value) in dst.iter_mut().zip(src) {
                *slot += weight * value;
            }
        }
    });
    out
}

/// Sharpens the luminance of a 3-channel linear image in place, as described in the module docs.
/// An amount of 0 (or one that is not a positive number) returns at once and changes nothing.
pub fn apply_sharpen(image: &mut ImageF32, sharpen: &Sharpen) -> Result<(), AppError> {
    require_rgb(image, "sharpen")?;
    let strength = sharpen.amount / 100.0;
    // (The fields are public: a hand-made image with no pixels has nothing to sharpen, and a
    // chunk size of 0 would panic.)
    if strength.is_nan() || strength <= 0.0 || image.width == 0 || image.height == 0 {
        return Ok(());
    }
    let sigma = if sharpen.radius_px.is_finite() && sharpen.radius_px > 0.0 {
        sharpen.radius_px
    } else {
        gtp_core::recipe::DEFAULT_SHARPEN_RADIUS_PX
    };
    let (width, height) = (image.width as usize, image.height as usize);

    let (pixels, _) = image.data.as_chunks_mut::<3>();
    pixels.par_iter_mut().for_each(|pixel| {
        for c in pixel.iter_mut() {
            *c = if c.is_finite() {
                c.clamp(0.0, MAX_SAMPLE)
            } else {
                0.0
            };
        }
    });
    let encoded: Vec<f32> = pixels
        .par_iter()
        .map(|pixel| luma(*pixel).max(0.0).powf(1.0 / SHARPEN_GAMMA))
        .collect();

    let weights = kernel(sigma);
    let blurred = blur_columns(
        &blur_rows(&encoded, width, &weights),
        width,
        height,
        &weights,
    );

    pixels
        .par_iter_mut()
        .zip(encoded.par_iter().zip(blurred.par_iter()))
        .for_each(|(pixel, (&edge, &soft))| {
            let y = luma(*pixel);
            if y <= BLACK {
                return;
            }
            let sharper = (edge + strength * (edge - soft)).max(0.0);
            let scale = sharper.powf(SHARPEN_GAMMA) / y;
            for c in pixel.iter_mut() {
                *c = (*c * scale).clamp(0.0, MAX_SAMPLE);
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn sharpen(amount: f32, radius_px: f32) -> Sharpen {
        Sharpen { amount, radius_px }
    }

    /// A grey image `width` x `height` whose columns before `edge` are `left` and the rest `right`.
    fn step(width: u32, height: u32, edge: u32, left: f32, right: f32) -> ImageF32 {
        let mut data = Vec::new();
        for _ in 0..height {
            for x in 0..width {
                let v = if x < edge { left } else { right };
                data.extend_from_slice(&[v, v, v]);
            }
        }
        ImageF32::new(width, height, 3, data).unwrap()
    }

    fn at(image: &ImageF32, x: u32, y: u32) -> [f32; 3] {
        let i = ((y * image.width + x) * 3) as usize;
        [image.data[i], image.data[i + 1], image.data[i + 2]]
    }

    #[test]
    fn the_kernel_sums_to_one_and_is_symmetric() {
        for sigma in [0.3, 1.0, 2.5, 3.0] {
            let k = kernel(sigma);
            assert!((k.iter().sum::<f32>() - 1.0).abs() < 1e-6, "{sigma}");
            assert_eq!(k.len() % 2, 1);
            for i in 0..k.len() / 2 {
                assert!((k[i] - k[k.len() - 1 - i]).abs() < 1e-7);
            }
            assert_eq!(k.len(), 2 * (3.0 * sigma).ceil().max(1.0) as usize + 1);
        }
    }

    #[test]
    fn an_amount_of_zero_or_less_changes_nothing_at_all() {
        let original = step(20, 8, 10, 0.2, 0.6);
        for amount in [0.0, -10.0, f32::NAN] {
            let mut image = original.clone();
            apply_sharpen(&mut image, &sharpen(amount, 2.0)).unwrap();
            assert_eq!(image, original, "{amount}");
        }
    }

    #[test]
    fn a_flat_image_stays_flat_at_any_amount_and_radius() {
        for (amount, radius) in [(25.0, 0.3), (100.0, 1.0), (150.0, 3.0)] {
            let mut image = step(30, 20, 0, 0.4, 0.4);
            apply_sharpen(&mut image, &sharpen(amount, radius)).unwrap();
            for chunk in image.data.chunks(3) {
                for &v in chunk {
                    assert!((v - 0.4).abs() < 1e-5, "{amount} {radius}: {v}");
                }
            }
        }
        // Also a flat coloured one.
        let mut colour = ImageF32::new(9, 9, 3, [0.3, 0.5, 0.2].repeat(81)).unwrap();
        apply_sharpen(&mut colour, &sharpen(150.0, 2.0)).unwrap();
        for chunk in colour.data.chunks(3) {
            assert!((chunk[0] - 0.3).abs() < 1e-5 && (chunk[1] - 0.5).abs() < 1e-5);
        }
    }

    #[test]
    fn an_edge_gets_a_dark_halo_on_the_dark_side_and_a_light_one_on_the_light_side() {
        let mut image = step(40, 6, 20, 0.2, 0.6);
        apply_sharpen(&mut image, &sharpen(100.0, 1.5)).unwrap();
        assert!(
            at(&image, 19, 3)[0] < 0.2,
            "undershoot {}",
            at(&image, 19, 3)[0]
        );
        assert!(
            at(&image, 20, 3)[0] > 0.6,
            "overshoot {}",
            at(&image, 20, 3)[0]
        );
        // Far from the edge nothing changed.
        assert!((at(&image, 2, 3)[0] - 0.2).abs() < 1e-5);
        assert!((at(&image, 37, 3)[0] - 0.6).abs() < 1e-5);
        // The edge is symmetric about its position in the encoding, so rows are all alike.
        assert_eq!(at(&image, 19, 0), at(&image, 19, 5));
    }

    #[test]
    fn a_larger_amount_sharpens_more_and_a_larger_radius_reaches_further() {
        let overshoot = |amount: f32, radius: f32| {
            let mut image = step(40, 6, 20, 0.2, 0.6);
            apply_sharpen(&mut image, &sharpen(amount, radius)).unwrap();
            at(&image, 20, 3)[0] - 0.6
        };
        let (a, b, c) = (
            overshoot(30.0, 1.0),
            overshoot(80.0, 1.0),
            overshoot(150.0, 1.0),
        );
        assert!(0.0 < a && a < b && b < c, "{a} {b} {c}");

        let reach = |radius: f32| {
            let mut image = step(60, 4, 30, 0.2, 0.6);
            apply_sharpen(&mut image, &sharpen(100.0, radius)).unwrap();
            (0..30)
                .filter(|x| (at(&image, *x, 2)[0] - 0.2).abs() > 1e-4)
                .count()
        };
        assert!(reach(0.3) < reach(1.5) && reach(1.5) < reach(3.0));
        assert!(
            reach(0.3) <= 2,
            "a tiny radius touches only the pixel at the edge"
        );
    }

    #[test]
    fn hue_and_saturation_are_kept() {
        // A coloured step: the ratios of the channels of every pixel are unchanged.
        let mut data = Vec::new();
        for _ in 0..4 {
            for x in 0..24 {
                let (r, g, b) = if x < 12 {
                    (0.5, 0.2, 0.1)
                } else {
                    (0.2, 0.6, 0.7)
                };
                data.extend_from_slice(&[r, g, b]);
            }
        }
        let original = ImageF32::new(24, 4, 3, data).unwrap();
        let mut image = original.clone();
        apply_sharpen(&mut image, &sharpen(120.0, 1.0)).unwrap();
        let mut changed = false;
        for (after, before) in image.data.chunks(3).zip(original.data.chunks(3)) {
            changed |= (after[0] - before[0]).abs() > 1e-3;
            assert!((after[0] / after[1] - before[0] / before[1]).abs() < 1e-4);
            assert!((after[2] / after[1] - before[2] / before[1]).abs() < 1e-4);
        }
        assert!(changed, "the edge was sharpened");
    }

    #[test]
    fn black_stays_black_and_the_result_is_never_negative() {
        let mut image = step(30, 6, 15, 0.0, 0.9);
        apply_sharpen(&mut image, &sharpen(150.0, 3.0)).unwrap();
        assert!(image.data.iter().all(|v| v.is_finite() && *v >= 0.0));
        assert_eq!(at(&image, 0, 0), [0.0; 3]);
    }

    #[test]
    fn it_works_on_images_smaller_than_the_kernel() {
        for (w, h) in [(1, 1), (1, 7), (7, 1), (2, 2)] {
            let mut image = ImageF32::new(w, h, 3, vec![0.5; (w * h * 3) as usize]).unwrap();
            apply_sharpen(&mut image, &sharpen(150.0, 3.0)).unwrap();
            assert!(image.data.iter().all(|v| (v - 0.5).abs() < 1e-5), "{w}x{h}");
        }
    }

    #[test]
    fn an_image_with_no_pixels_is_left_alone() {
        let mut empty = ImageF32 {
            width: 0,
            height: 0,
            channels: 3,
            data: Vec::new(),
        };
        apply_sharpen(&mut empty, &sharpen(100.0, 1.0)).unwrap();
        assert!(empty.data.is_empty());
    }

    #[test]
    fn highlights_above_one_keep_their_headroom() {
        let mut image = step(30, 4, 15, 0.3, 2.5);
        apply_sharpen(&mut image, &sharpen(100.0, 1.0)).unwrap();
        assert!(at(&image, 15, 2)[0] > 2.5);
        assert!(image.data.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn the_result_is_the_same_on_one_thread_as_on_many() {
        let original = step(70, 31, 33, 0.15, 0.7);
        let mut many = original.clone();
        apply_sharpen(&mut many, &sharpen(90.0, 2.2)).unwrap();
        let mut one = original;
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(|| apply_sharpen(&mut one, &sharpen(90.0, 2.2)).unwrap());
        assert_eq!(one, many);
    }

    #[test]
    fn an_image_that_is_not_rgb_is_refused() {
        let mut mono = ImageF32::new(2, 2, 1, vec![0.5; 4]).unwrap();
        let err = apply_sharpen(&mut mono, &sharpen(50.0, 1.0)).unwrap_err();
        assert_eq!(err.code, gtp_core::ErrorCode::InvalidInput);
    }

    proptest! {
        #[test]
        fn the_output_is_finite_and_not_negative_for_any_input(
            data in proptest::collection::vec(proptest::num::f32::ANY, 3..90),
            amount in 0f32..=150.0, radius in 0.3f32..=3.0,
        ) {
            let mut data = data;
            data.truncate(data.len() / 3 * 3);
            let pixels = data.len() / 3;
            let mut image = ImageF32::new(pixels as u32, 1, 3, data).unwrap();
            apply_sharpen(&mut image, &sharpen(amount, radius)).unwrap();
            prop_assert!(image.data.iter().all(|v| v.is_finite() && *v >= 0.0));
        }
    }
}
