//! Bilinear demosaic of a [`RawLinear`] mosaic to linear f32 (black subtracted, divided by the
//! range), either all three channels or just one.
//!
//! The interpolation reflects at the edges, which keeps the colour of the neighbours right (a
//! clamp would reuse the pixel itself), so a field of one value comes out as that value everywhere
//! and a linear ramp is exact away from the border.

use gtp_core::{AppError, ImageF32};

use super::RawLinear;

/// `index` reflected into `0..len`.
fn reflect(index: isize, len: usize) -> usize {
    let last = len as isize - 1;
    let reflected = if index < 0 {
        -index
    } else if index > last {
        2 * last - index
    } else {
        index
    };
    reflected.clamp(0, last) as usize
}

/// The scaled sample at (x, y): black removed, divided by the range, not below 0.
struct Scaled<'a> {
    raw: &'a RawLinear,
    width: usize,
    height: usize,
}

impl Scaled<'_> {
    fn at(&self, x: isize, y: isize) -> f32 {
        let (x, y) = (reflect(x, self.width), reflect(y, self.height));
        let black = self.raw.black[((y & 1) << 1) | (x & 1)];
        let value = f32::from(self.raw.data[y * self.width + x]);
        ((value - black) / (self.raw.white - black)).max(0.0)
    }
}

/// One channel (0 red, 1 green, 2 blue) of the mosaic by bilinear interpolation.
fn interpolate(raw: &RawLinear, channel: usize) -> Vec<f32> {
    let (width, height) = (raw.width as usize, raw.height as usize);
    let samples = Scaled { raw, width, height };
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height as isize {
        for x in 0..width as isize {
            let here = raw.cfa.color_at(x as usize, y as usize);
            let value = if here == channel {
                samples.at(x, y)
            } else if channel == 1 {
                // A red or blue pixel: its four neighbours are all green.
                (samples.at(x - 1, y)
                    + samples.at(x + 1, y)
                    + samples.at(x, y - 1)
                    + samples.at(x, y + 1))
                    * 0.25
            } else if here == 1 {
                // A green pixel: the wanted colour is next to it on the row or on the column.
                if raw.cfa.color_at(x as usize + 1, y as usize) == channel {
                    (samples.at(x - 1, y) + samples.at(x + 1, y)) * 0.5
                } else {
                    (samples.at(x, y - 1) + samples.at(x, y + 1)) * 0.5
                }
            } else {
                // The opposite colour of red and blue: the four diagonal neighbours.
                (samples.at(x - 1, y - 1)
                    + samples.at(x + 1, y - 1)
                    + samples.at(x - 1, y + 1)
                    + samples.at(x + 1, y + 1))
                    * 0.25
            };
            out.push(value);
        }
    }
    out
}

/// One channel of the mosaic as a 1-channel image: `channel` is 0 for red, 1 for green, 2 for blue.
/// A third of the work of [`demosaic_bilinear`], which is all a triplet needs from each exposure.
pub fn demosaic_channel(raw: &RawLinear, channel: usize) -> Result<ImageF32, AppError> {
    raw.validate()?;
    if channel > 2 {
        return Err(AppError::new(
            gtp_core::ErrorCode::InvalidInput,
            "A channel is 0 (red), 1 (green) or 2 (blue).",
        )
        .with_details(format!("channel {channel}")));
    }
    ImageF32::new(raw.width, raw.height, 1, interpolate(raw, channel))
}

/// The mosaic as a 3-channel image (red, green, blue).
pub fn demosaic_bilinear(raw: &RawLinear) -> Result<ImageF32, AppError> {
    raw.validate()?;
    let planes: Vec<Vec<f32>> = (0..3).map(|c| interpolate(raw, c)).collect();
    let data = (0..planes[0].len())
        .flat_map(|i| [planes[0][i], planes[1][i], planes[2][i]])
        .collect();
    ImageF32::new(raw.width, raw.height, 3, data)
}

#[cfg(test)]
mod tests {
    use super::super::Cfa;
    use super::*;

    /// A mosaic of `cfa` whose sample at (x, y) of colour c is `value(c, x, y)`.
    fn mosaic(
        cfa: Cfa,
        w: u32,
        h: u32,
        black: f32,
        white: f32,
        value: impl Fn(usize, f32, f32) -> f32,
    ) -> RawLinear {
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let c = cfa.color_at(x as usize, y as usize);
                data.push(value(c, x as f32, y as f32).round() as u16);
            }
        }
        RawLinear {
            width: w,
            height: h,
            cfa,
            black: [black; 4],
            white,
            data,
        }
    }

    const PATTERNS: [Cfa; 4] = [Cfa::RGGB, Cfa::BGGR, Cfa::GRBG, Cfa::GBRG];

    fn pixel(image: &ImageF32, x: usize, y: usize) -> [f32; 3] {
        let at = (y * image.width as usize + x) * 3;
        [image.data[at], image.data[at + 1], image.data[at + 2]]
    }

    #[test]
    fn a_field_of_one_value_per_colour_comes_out_exactly_everywhere() {
        for cfa in PATTERNS {
            let raw = mosaic(cfa, 12, 10, 0.0, 10_000.0, |c, _, _| {
                [2_000.0, 5_000.0, 8_000.0][c]
            });
            let image = demosaic_bilinear(&raw).unwrap();
            for y in 0..10 {
                for x in 0..12 {
                    assert_eq!(pixel(&image, x, y), [0.2, 0.5, 0.8], "{cfa:?} at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn a_linear_ramp_is_exact_away_from_the_border() {
        for cfa in PATTERNS {
            // Each colour its own ramp in x and y, whole numbers so the samples are exact.
            let ramp = |c: usize, x: f32, y: f32| match c {
                0 => 1_000.0 + 40.0 * x + 20.0 * y,
                1 => 3_000.0 + 10.0 * x + 30.0 * y,
                _ => 6_000.0 + 20.0 * x + 10.0 * y,
            };
            let raw = mosaic(cfa, 16, 14, 0.0, 10_000.0, ramp);
            let image = demosaic_bilinear(&raw).unwrap();
            for y in 1..13 {
                for x in 1..15 {
                    let got = pixel(&image, x, y);
                    for (c, value) in got.iter().enumerate() {
                        let want = ramp(c, x as f32, y as f32) / 10_000.0;
                        assert!(
                            (value - want).abs() < 1e-5,
                            "{cfa:?} channel {c} at {x},{y}: {} vs {want}",
                            got[c]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn black_is_removed_and_the_range_scales_to_one() {
        let raw = mosaic(Cfa::RGGB, 8, 8, 100.0, 1_100.0, |c, _, _| {
            [100.0, 600.0, 1_100.0][c]
        });
        let image = demosaic_bilinear(&raw).unwrap();
        assert_eq!(pixel(&image, 4, 4), [0.0, 0.5, 1.0]);
        // Below black is not negative.
        let dark = mosaic(Cfa::RGGB, 8, 8, 100.0, 1_100.0, |_, _, _| 20.0);
        assert!(demosaic_bilinear(&dark)
            .unwrap()
            .data
            .iter()
            .all(|v| *v == 0.0));
    }

    #[test]
    fn one_channel_equals_that_channel_of_the_full_demosaic() {
        let raw = mosaic(Cfa::RGGB, 20, 18, 0.0, 15_520.0, |c, x, y| {
            500.0 * (c as f32 + 1.0) + 7.0 * x + 3.0 * y
        });
        let full = demosaic_bilinear(&raw).unwrap();
        for c in 0..3 {
            let plane = demosaic_channel(&raw, c).unwrap();
            assert_eq!(plane.channels, 1);
            for (i, sample) in plane.data.iter().enumerate() {
                assert_eq!(*sample, full.data[i * 3 + c], "channel {c} sample {i}");
            }
        }
    }

    #[test]
    fn a_shape_that_cannot_be_demosaiced_is_refused() {
        let good = mosaic(Cfa::RGGB, 4, 4, 0.0, 100.0, |_, _, _| 10.0);
        assert!(demosaic_bilinear(&good).is_ok());
        let short = RawLinear {
            data: vec![0; 15],
            ..good.clone()
        };
        let thin = RawLinear {
            width: 1,
            height: 16,
            ..good.clone()
        };
        let flat = RawLinear {
            white: 0.0,
            ..good.clone()
        };
        let odd = RawLinear {
            cfa: Cfa([0, 0, 1, 2]),
            ..good.clone()
        };
        let nan = RawLinear {
            white: f32::NAN,
            ..good.clone()
        };
        for (name, bad) in [
            ("short", short),
            ("thin", thin),
            ("flat", flat),
            ("odd", odd),
            ("nan", nan),
        ] {
            assert!(demosaic_bilinear(&bad).is_err(), "{name}");
        }
        assert!(demosaic_channel(&good, 3).is_err());
    }

    #[test]
    fn the_smallest_mosaic_works_and_a_pattern_not_starting_at_red_is_handled() {
        let raw = mosaic(Cfa::GBRG, 2, 2, 0.0, 100.0, |c, _, _| [10.0, 20.0, 30.0][c]);
        let image = demosaic_bilinear(&raw).unwrap();
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(pixel(&image, x, y), [0.1, 0.2, 0.3]);
            }
        }
    }
}
