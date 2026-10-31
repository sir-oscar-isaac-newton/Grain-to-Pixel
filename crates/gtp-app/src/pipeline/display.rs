//! Display conversion for previews and the histogram.
//!
//! The working space is linear Rec.2020 primaries (docs/specification.md section 4.2); previews
//! assume an sRGB display. [`to_rgba8_srgb`] converts the primaries with a fixed 3x3 matrix, clips
//! to `[0, 1]` and applies the sRGB transfer curve. Colour-managed export with ICC profiles is a
//! separate step (P-04).

use std::sync::OnceLock;

use gtp_core::{AppError, ErrorCode, ImageF32};
use rayon::prelude::*;

/// Linear Rec.2020 to linear Rec.709/sRGB primaries (D65).
const REC2020_TO_SRGB: [[f32; 3]; 3] = [
    [1.660_491, -0.587_641, -0.072_850],
    [-0.124_551, 1.132_9, -0.008_349],
    [-0.018_151, -0.100_579, 1.118_73],
];

fn srgb_encode(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// The 8-bit sRGB value of a linear value in `[0, 1]`, worked out the slow way.
fn encode_byte(linear: f32) -> u8 {
    (srgb_encode(linear) * 255.0).round() as u8
}

/// For each level `k` from 1 to 255, the smallest linear value (in `[0, 1]`) that [`encode_byte`]
/// turns into `k` or more. Found by bisection on the bit patterns of the floats (for non-negative
/// floats, the order of the bits is the order of the values), so a value maps to a byte by
/// counting how many of these it reaches, with exactly the result of [`encode_byte`] and without a
/// power function per channel.
fn level_thresholds() -> &'static [f32; 255] {
    static THRESHOLDS: OnceLock<[f32; 255]> = OnceLock::new();
    THRESHOLDS.get_or_init(|| {
        let mut found = [0.0f32; 255];
        for (n, slot) in found.iter_mut().enumerate() {
            let level = (n + 1) as u8;
            // The first bit pattern in 0 to 1.0 whose byte is `level` or more.
            let (mut low, mut high) = (0u32, 1.0f32.to_bits());
            while low < high {
                let mid = low + (high - low) / 2;
                if encode_byte(f32::from_bits(mid)) >= level {
                    high = mid;
                } else {
                    low = mid + 1;
                }
            }
            *slot = f32::from_bits(low);
        }
        found
    })
}

/// [`encode_byte`], by lookup: how many levels the value reaches.
fn byte_of(linear: f32, thresholds: &[f32; 255]) -> u8 {
    thresholds.partition_point(|level| *level <= linear) as u8
}

/// Pixels converted by one task: enough work to be worth handing to another thread.
const PIXELS_PER_TASK: usize = 4096;

/// Converts a linear Rec.2020 image to 8-bit sRGB RGBA (alpha 255), row-major. Values outside the
/// sRGB gamut or above 1.0 are clipped; values that are not finite become black.
pub fn to_rgba8_srgb(image: &ImageF32) -> Result<Vec<u8>, AppError> {
    if image.channels != 3 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Display conversion needs a 3-channel image.",
        )
        .with_details(format!("{} channels", image.channels)));
    }
    let thresholds = level_thresholds();
    let (pixels, _) = image.data.as_chunks::<3>();
    let mut out = vec![0u8; pixels.len() * 4];
    out.par_chunks_mut(PIXELS_PER_TASK * 4)
        .zip(pixels.par_chunks(PIXELS_PER_TASK))
        .for_each(|(rgba_run, rgb_run)| {
            let (rgba_run, _) = rgba_run.as_chunks_mut::<4>();
            for (rgba, rgb) in rgba_run.iter_mut().zip(rgb_run) {
                for (slot, row) in rgba.iter_mut().zip(REC2020_TO_SRGB) {
                    let linear = row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2];
                    let clipped = if linear.is_finite() {
                        linear.clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    *slot = byte_of(clipped, thresholds);
                }
                rgba[3] = 255;
            }
        });
    Ok(out)
}

/// 256-bin histograms of 8-bit display values, as the Develop view shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Histogram {
    pub red: [u32; 256],
    pub green: [u32; 256],
    pub blue: [u32; 256],
    /// Rec.709 luma of the display values.
    pub luma: [u32; 256],
}

/// Histogram of an RGBA8 buffer (as returned by [`to_rgba8_srgb`]).
pub fn histogram(rgba: &[u8]) -> Histogram {
    let mut h = Histogram {
        red: [0; 256],
        green: [0; 256],
        blue: [0; 256],
        luma: [0; 256],
    };
    let (pixels, _) = rgba.as_chunks::<4>();
    for [r, g, b, _] in pixels {
        h.red[usize::from(*r)] += 1;
        h.green[usize::from(*g)] += 1;
        h.blue[usize::from(*b)] += 1;
        let y = 0.2126 * f32::from(*r) + 0.7152 * f32::from(*g) + 0.0722 * f32::from(*b);
        h.luma[usize::from(y.round().clamp(0.0, 255.0) as u8)] += 1;
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba_of(rgb: [f32; 3]) -> [u8; 4] {
        let image = ImageF32::new(1, 1, 3, rgb.to_vec()).unwrap();
        let out = to_rgba8_srgb(&image).unwrap();
        [out[0], out[1], out[2], out[3]]
    }

    #[test]
    fn black_white_and_mid_grey_land_where_srgb_puts_them() {
        assert_eq!(rgba_of([0.0; 3]), [0, 0, 0, 255]);
        assert_eq!(rgba_of([1.0; 3]), [255, 255, 255, 255]);
        // Linear 0.18 is sRGB 0.461, which is 118 of 255.
        assert_eq!(rgba_of([0.18; 3]), [118, 118, 118, 255]);
    }

    #[test]
    fn grey_stays_grey_through_the_primaries_matrix() {
        for row in REC2020_TO_SRGB {
            assert!(
                (row.iter().sum::<f32>() - 1.0).abs() < 1e-3,
                "white must map to white"
            );
        }
    }

    #[test]
    fn out_of_range_and_non_finite_values_are_clipped() {
        assert_eq!(rgba_of([4.0; 3]), [255, 255, 255, 255]);
        assert_eq!(rgba_of([-1.0, f32::NAN, f32::INFINITY]), [0, 0, 0, 255]);
        // Saturated Rec.2020 red is outside sRGB: red clips high, green and blue clip at 0.
        assert_eq!(rgba_of([1.0, 0.0, 0.0]), [255, 0, 0, 255]);
    }

    #[test]
    fn the_histogram_counts_every_pixel_once_per_channel() {
        let rgba = [10u8, 20, 30, 255, 10, 200, 30, 255, 255, 255, 255, 255];
        let h = histogram(&rgba);
        assert_eq!(h.red[10], 2);
        assert_eq!(h.green[20] + h.green[200], 2);
        assert_eq!(h.blue[30], 2);
        assert_eq!(h.luma[255], 1);
        for bins in [&h.red, &h.green, &h.blue, &h.luma] {
            assert_eq!(bins.iter().sum::<u32>(), 3);
        }
    }

    #[test]
    fn a_non_rgb_image_is_rejected() {
        let gray = ImageF32::zeros(2, 2, 1).unwrap();
        assert_eq!(
            to_rgba8_srgb(&gray).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    // The conversion by lookup (P-17) must give the byte the formula gives, for every value.

    #[test]
    fn the_thresholds_rise_and_cover_every_level() {
        let thresholds = level_thresholds();
        assert!(thresholds[0] > 0.0, "black stays black");
        assert!(thresholds.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(thresholds[254] <= 1.0, "white is reached by 1.0");
        assert_eq!(byte_of(0.0, thresholds), 0);
        assert_eq!(byte_of(1.0, thresholds), 255);
        // Each level starts exactly where the formula changes its answer.
        for (n, threshold) in thresholds.iter().enumerate() {
            let level = (n + 1) as u8;
            assert!(encode_byte(*threshold) >= level, "level {level}");
            let below = f32::from_bits(threshold.to_bits() - 1);
            assert!(encode_byte(below) < level, "level {level}");
        }
    }

    #[test]
    fn the_lookup_gives_the_byte_of_the_formula_around_every_threshold_and_across_the_range() {
        let thresholds = level_thresholds();
        let check = |bits: u32| {
            let value = f32::from_bits(bits);
            assert_eq!(
                byte_of(value, thresholds),
                encode_byte(value),
                "linear {value:e} (bits {bits:#x})"
            );
        };
        for threshold in thresholds {
            let at = threshold.to_bits();
            for bits in at.saturating_sub(64)..=at + 64 {
                check(bits);
            }
        }
        let top = 1.0f32.to_bits();
        for bits in (0..=top).step_by(997) {
            check(bits);
        }
        check(top);
    }

    /// Every float from 0 to 1, a billion of them: slow, so only on request. Run with
    /// `cargo test -p gtp-app --release -- --ignored every_float`.
    #[test]
    #[ignore = "exhaustive, about a minute"]
    fn every_float_between_zero_and_one_gives_the_byte_of_the_formula() {
        let thresholds = level_thresholds();
        let wrong = (0..=1.0f32.to_bits())
            .into_par_iter()
            .filter(|bits| {
                let value = f32::from_bits(*bits);
                byte_of(value, thresholds) != encode_byte(value)
            })
            .count();
        assert_eq!(wrong, 0);
    }

    /// `to_rgba8_srgb` as it was before the lookup: one formula per channel.
    fn reference_rgba(image: &ImageF32) -> Vec<u8> {
        let (pixels, _) = image.data.as_chunks::<3>();
        let mut out = vec![0u8; pixels.len() * 4];
        let (out_pixels, _) = out.as_chunks_mut::<4>();
        for (rgba, rgb) in out_pixels.iter_mut().zip(pixels) {
            for (slot, row) in rgba.iter_mut().zip(REC2020_TO_SRGB) {
                let linear = row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2];
                let clipped = if linear.is_finite() {
                    linear.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                *slot = encode_byte(clipped);
            }
            rgba[3] = 255;
        }
        out
    }

    #[test]
    fn whole_images_convert_to_the_same_bytes_as_before_also_across_task_boundaries() {
        let special = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -0.0,
            0.0,
            1.0,
            4.0,
            -2.0,
        ];
        // Sizes around the task size, so runs end mid-row, and one that is a single pixel.
        for (w, h) in [(1u32, 1u32), (70, 61), (64, 64), (65, 64), (301, 29)] {
            let mut state = w * 31 + h;
            let data = (0..w * h * 3)
                .map(|i| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    if i % 97 == 0 {
                        special[(state % 8) as usize]
                    } else {
                        (state % 13_000) as f32 / 10_000.0 - 0.1
                    }
                })
                .collect();
            let image = ImageF32::new(w, h, 3, data).unwrap();
            assert_eq!(
                to_rgba8_srgb(&image).unwrap(),
                reference_rgba(&image),
                "{w}x{h}"
            );
        }
    }
}
