//! Display conversion for previews and the histogram.
//!
//! The working space is linear Rec.2020 primaries (docs/specification.md section 4.2); previews
//! assume an sRGB display. [`to_rgba8_srgb`] converts the primaries with a fixed 3x3 matrix, clips
//! to `[0, 1]` and applies the sRGB transfer curve. Colour-managed export with ICC profiles is a
//! separate step (P-04).

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
    let (pixels, _) = image.data.as_chunks::<3>();
    let mut out = vec![0u8; pixels.len() * 4];
    out.par_chunks_mut(4)
        .zip(pixels.par_iter())
        .for_each(|(rgba, rgb)| {
            for (slot, row) in rgba.iter_mut().zip(REC2020_TO_SRGB) {
                let linear = row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2];
                let clipped = if linear.is_finite() {
                    linear.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                *slot = (srgb_encode(clipped) * 255.0).round() as u8;
            }
            rgba[3] = 255;
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
}
