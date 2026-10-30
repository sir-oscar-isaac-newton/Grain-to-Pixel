//! Geometry stage (docs/specification.md section 4.2 step 8): crop, straighten, turn and flip,
//! with at most one resample.
//!
//! The Recipe's [`Geometry`] is read in this order:
//!
//! 1. **Crop**, fractions of the source image, rounded to whole pixels.
//! 2. **Straighten** by `angle_deg` (positive turns the picture clockwise) around the crop centre.
//!    The output is the largest rectangle with the crop's aspect ratio that fits inside the rotated
//!    crop, so there are no empty corners and every sample position lies inside the crop.
//! 3. **Turn** by `rot90` quarter turns clockwise.
//! 4. **Flip** horizontally and vertically, in the final orientation.
//!
//! With an angle of exactly 0 every output pixel is a copy of a source pixel. Otherwise each output
//! pixel is sampled once from the source with a Catmull-Rom bicubic filter at the position given by
//! the composed transform of all four steps; the turns and flips never cause a second resample.
//! Taps that fall outside the image repeat its edge pixels, and results are limited to be at
//! least 0 (bicubic can overshoot).

use gtp_core::recipe::Geometry;
use gtp_core::{AppError, ErrorCode, ImageF32};

fn invalid(message: &str, details: String) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message).with_details(details)
}

/// Everything derived from a source size and a [`Geometry`].
struct Plan {
    /// Crop origin and size in source pixels.
    x0: usize,
    y0: usize,
    crop_w: usize,
    crop_h: usize,
    /// Size after straightening, before turns and flips.
    straight_w: usize,
    straight_h: usize,
    /// Final size.
    out_w: usize,
    out_h: usize,
    quarter_turns: u8,
    flip_h: bool,
    flip_v: bool,
    /// Sine and cosine of the straighten angle; `None` for exactly 0.
    rotation: Option<(f64, f64)>,
}

fn plan(width: u32, height: u32, geometry: &Geometry) -> Result<Plan, AppError> {
    let crop = geometry.crop;
    let fractions = [crop.x, crop.y, crop.w, crop.h];
    if fractions.iter().any(|v| !v.is_finite()) || !geometry.angle_deg.is_finite() {
        return Err(invalid(
            "The crop and angle must be numbers.",
            format!("{crop:?}, angle {}", geometry.angle_deg),
        ));
    }
    let (w, h) = (f64::from(width), f64::from(height));
    let edge =
        |fraction: f32, size: f64| (f64::from(fraction) * size).round().clamp(0.0, size) as usize;
    let (x0, y0) = (edge(crop.x, w), edge(crop.y, h));
    let x1 = edge(crop.x + crop.w, w).max(x0);
    let y1 = edge(crop.y + crop.h, h).max(y0);
    let (crop_w, crop_h) = (x1 - x0, y1 - y0);
    if crop_w == 0 || crop_h == 0 {
        return Err(invalid(
            "The crop is empty.",
            format!("{crop:?} on {width}x{height}"),
        ));
    }

    let (straight_w, straight_h, rotation) = if geometry.angle_deg == 0.0 {
        (crop_w, crop_h, None)
    } else {
        let theta = f64::from(geometry.angle_deg).to_radians();
        let (sin, cos) = theta.sin_cos();
        let (a, b) = (crop_w as f64 / 2.0, crop_h as f64 / 2.0);
        // The corners (+-s*a, +-s*b) of the output, turned by theta, must stay inside the crop.
        let fit_x = a / (a * cos.abs() + b * sin.abs());
        let fit_y = b / (a * sin.abs() + b * cos.abs());
        let scale = fit_x.min(fit_y).min(1.0);
        let shrink = |size: usize| ((scale * size as f64 + 1e-6).floor() as usize).max(1);
        (shrink(crop_w), shrink(crop_h), Some((sin, cos)))
    };

    let quarter_turns = geometry.rot90.rem_euclid(4) as u8;
    let (out_w, out_h) = if quarter_turns % 2 == 1 {
        (straight_h, straight_w)
    } else {
        (straight_w, straight_h)
    };
    Ok(Plan {
        x0,
        y0,
        crop_w,
        crop_h,
        straight_w,
        straight_h,
        out_w,
        out_h,
        quarter_turns,
        flip_h: geometry.flip_h,
        flip_v: geometry.flip_v,
        rotation,
    })
}

/// The size of the image [`apply_geometry`] would produce, without doing the work.
pub fn output_size(width: u32, height: u32, geometry: &Geometry) -> Result<(u32, u32), AppError> {
    let p = plan(width, height, geometry)?;
    // Both are at most the source size, so they fit in u32.
    Ok((p.out_w as u32, p.out_h as u32))
}

impl Plan {
    /// Where final pixel `(i, j)` comes from in the straightened image.
    fn straight_pixel(&self, i: usize, j: usize) -> (usize, usize) {
        let i = if self.flip_h { self.out_w - 1 - i } else { i };
        let j = if self.flip_v { self.out_h - 1 - j } else { j };
        let (sw, sh) = (self.straight_w, self.straight_h);
        match self.quarter_turns {
            1 => (j, sh - 1 - i),
            2 => (sw - 1 - i, sh - 1 - j),
            3 => (sw - 1 - j, i),
            _ => (i, j),
        }
    }

    /// The source position (pixel-centre coordinates) that straightened pixel `(x, y)` shows.
    fn source_position(&self, sin: f64, cos: f64, x: usize, y: usize) -> (f64, f64) {
        let u = x as f64 + 0.5 - self.straight_w as f64 / 2.0;
        let v = y as f64 + 0.5 - self.straight_h as f64 / 2.0;
        let cx = self.x0 as f64 + self.crop_w as f64 / 2.0;
        let cy = self.y0 as f64 + self.crop_h as f64 / 2.0;
        (cx + u * cos + v * sin, cy - u * sin + v * cos)
    }
}

/// Catmull-Rom weights for the four taps around a position with fractional part `t`.
fn weights(t: f64) -> [f64; 4] {
    let (t2, t3) = (t * t, t * t * t);
    [
        (-t3 + 2.0 * t2 - t) / 2.0,
        (3.0 * t3 - 5.0 * t2 + 2.0) / 2.0,
        (-3.0 * t3 + 4.0 * t2 + t) / 2.0,
        (t3 - t2) / 2.0,
    ]
}

/// Applies the Recipe's geometry and returns the new image (any channel count). See the module
/// docs for the order of the steps and for how the output size is chosen.
pub fn apply_geometry(image: &ImageF32, geometry: &Geometry) -> Result<ImageF32, AppError> {
    let p = plan(image.width, image.height, geometry)?;
    let channels = usize::from(image.channels);
    let (src_w, src_h) = (image.width as usize, image.height as usize);
    let mut out = vec![0.0f32; p.out_w * p.out_h * channels];

    for j in 0..p.out_h {
        for i in 0..p.out_w {
            let (sx, sy) = p.straight_pixel(i, j);
            let target = (j * p.out_w + i) * channels;
            let Some((sin, cos)) = p.rotation else {
                let from = ((p.y0 + sy) * src_w + p.x0 + sx) * channels;
                out[target..target + channels].copy_from_slice(&image.data[from..from + channels]);
                continue;
            };

            let (px, py) = p.source_position(sin, cos, sx, sy);
            let (fx, fy) = (px - 0.5, py - 0.5);
            let (ix, iy) = (fx.floor(), fy.floor());
            let (wx, wy) = (weights(fx - ix), weights(fy - iy));
            for (c, slot) in out[target..target + channels].iter_mut().enumerate() {
                let mut sum = 0.0;
                for (dy, weight_y) in wy.iter().enumerate() {
                    let row = (iy as i64 + dy as i64 - 1).clamp(0, src_h as i64 - 1) as usize;
                    for (dx, weight_x) in wx.iter().enumerate() {
                        let col = (ix as i64 + dx as i64 - 1).clamp(0, src_w as i64 - 1) as usize;
                        sum += weight_x
                            * weight_y
                            * f64::from(image.data[(row * src_w + col) * channels + c]);
                    }
                }
                *slot = (sum as f32).max(0.0);
            }
        }
    }
    ImageF32::new(p.out_w as u32, p.out_h as u32, image.channels, out)
}

#[cfg(test)]
mod tests {
    use gtp_core::recipe::Crop;
    use proptest::prelude::*;

    use super::*;

    /// A 3-channel image whose first channel is a unique id per pixel.
    fn numbered(w: u32, h: u32) -> ImageF32 {
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let id = (y * w + x) as f32;
                data.extend([id, id + 0.25, id + 0.5]);
            }
        }
        ImageF32::new(w, h, 3, data).unwrap()
    }

    /// Channel 0 holds the pixel-centre x coordinate, channel 1 the y coordinate.
    fn ramp(w: u32, h: u32) -> ImageF32 {
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                data.extend([x as f32 + 0.5, y as f32 + 0.5, 0.0]);
            }
        }
        ImageF32::new(w, h, 3, data).unwrap()
    }

    fn at(image: &ImageF32, x: u32, y: u32) -> Vec<f32> {
        let c = usize::from(image.channels);
        let start = (y as usize * image.width as usize + x as usize) * c;
        image.data[start..start + c].to_vec()
    }

    fn geo(f: impl FnOnce(&mut Geometry)) -> Geometry {
        let mut g = Geometry::default();
        f(&mut g);
        g
    }

    fn crop(x: f32, y: f32, w: f32, h: f32) -> Crop {
        Crop { x, y, w, h }
    }

    #[test]
    fn the_default_geometry_is_the_identity() {
        let img = numbered(7, 5);
        assert_eq!(apply_geometry(&img, &Geometry::default()).unwrap(), img);
        assert_eq!(output_size(7, 5, &Geometry::default()).unwrap(), (7, 5));
    }

    #[test]
    fn four_quarter_turns_are_the_identity_and_turns_wrap() {
        let img = numbered(7, 5);
        let turn = geo(|g| g.rot90 = 1);
        let mut current = img.clone();
        for _ in 0..4 {
            current = apply_geometry(&current, &turn).unwrap();
        }
        assert_eq!(current, img);
        assert_eq!(
            apply_geometry(&img, &geo(|g| g.rot90 = -1)).unwrap(),
            apply_geometry(&img, &geo(|g| g.rot90 = 3)).unwrap()
        );
        assert_eq!(
            apply_geometry(&img, &geo(|g| g.rot90 = 6)).unwrap(),
            apply_geometry(&img, &geo(|g| g.rot90 = 2)).unwrap()
        );
    }

    #[test]
    fn flips_are_involutions_and_two_flips_make_a_half_turn() {
        let img = numbered(7, 5);
        for flip in [geo(|g| g.flip_h = true), geo(|g| g.flip_v = true)] {
            let once = apply_geometry(&img, &flip).unwrap();
            assert_ne!(once, img);
            assert_eq!(apply_geometry(&once, &flip).unwrap(), img);
        }
        let both = geo(|g| {
            g.flip_h = true;
            g.flip_v = true;
        });
        assert_eq!(
            apply_geometry(&img, &both).unwrap(),
            apply_geometry(&img, &geo(|g| g.rot90 = 2)).unwrap()
        );
    }

    #[test]
    fn a_marked_pixel_goes_where_a_turn_or_flip_sends_it() {
        // The pixel at the top-left of a 7x5 image.
        let img = numbered(7, 5);
        let marker = at(&img, 0, 0);
        let check = |g: Geometry, size: (u32, u32), at_xy: (u32, u32)| {
            let out = apply_geometry(&img, &g).unwrap();
            assert_eq!((out.width, out.height), size);
            assert_eq!(at(&out, at_xy.0, at_xy.1), marker, "{g:?}");
        };
        check(geo(|g| g.rot90 = 1), (5, 7), (4, 0)); // clockwise: top-left goes to top-right
        check(geo(|g| g.rot90 = 2), (7, 5), (6, 4));
        check(geo(|g| g.rot90 = 3), (5, 7), (0, 6));
        check(geo(|g| g.flip_h = true), (7, 5), (6, 0));
        check(geo(|g| g.flip_v = true), (7, 5), (0, 4));
        // Flips act on the turned image: turn first, then mirror left-right.
        check(
            geo(|g| {
                g.rot90 = 1;
                g.flip_h = true;
            }),
            (5, 7),
            (0, 0),
        );
    }

    #[test]
    fn a_crop_extracts_exactly_those_pixels() {
        let img = numbered(8, 8);
        let g = geo(|g| g.crop = crop(0.25, 0.5, 0.5, 0.25));
        let out = apply_geometry(&img, &g).unwrap();
        assert_eq!((out.width, out.height), (4, 2));
        assert_eq!(at(&out, 0, 0), at(&img, 2, 4));
        assert_eq!(at(&out, 3, 1), at(&img, 5, 5));
        assert_eq!(output_size(8, 8, &g).unwrap(), (4, 2));
    }

    #[test]
    fn a_crop_that_leaves_the_image_is_clamped_and_empty_ones_are_rejected() {
        let img = numbered(8, 8);
        let clamped = apply_geometry(&img, &geo(|g| g.crop = crop(0.5, 0.5, 2.0, 2.0))).unwrap();
        assert_eq!((clamped.width, clamped.height), (4, 4));
        assert_eq!(at(&clamped, 0, 0), at(&img, 4, 4));

        for bad in [
            crop(0.0, 0.0, 0.0, 1.0),
            crop(0.0, 0.0, 1.0, 0.0),
            crop(1.0, 0.0, 0.5, 1.0),
            crop(0.0, 0.0, 0.01, 1.0),
            crop(f32::NAN, 0.0, 1.0, 1.0),
            crop(0.0, 0.0, f32::INFINITY, 1.0),
        ] {
            let err = apply_geometry(&img, &geo(|g| g.crop = bad)).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{bad:?}");
        }
        let err = apply_geometry(&img, &geo(|g| g.angle_deg = f32::NAN)).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn straightening_samples_the_composed_transform_once() {
        // Catmull-Rom reproduces a linear ramp exactly, so the output of a ramp image is the
        // source coordinate each output pixel looks at.
        let img = ramp(60, 40);
        let angle = 20.0f32;
        // An interior crop (x 9..51, y 8..32, centre 30, 20): every filter tap is inside the image,
        // so the ramp is reproduced exactly. Near the image border the taps repeat the edge.
        let out = apply_geometry(
            &img,
            &geo(|g| {
                g.crop = crop(0.15, 0.2, 0.7, 0.6);
                g.angle_deg = angle;
            }),
        )
        .unwrap();
        let (sin, cos) = f64::from(angle).to_radians().sin_cos();
        let (ow, oh) = (f64::from(out.width), f64::from(out.height));
        for j in 0..out.height {
            for i in 0..out.width {
                let (u, v) = (f64::from(i) + 0.5 - ow / 2.0, f64::from(j) + 0.5 - oh / 2.0);
                let want_x = 30.0 + u * cos + v * sin;
                let want_y = 20.0 - u * sin + v * cos;
                let got = at(&out, i, j);
                assert!(
                    (f64::from(got[0]) - want_x).abs() < 1e-3,
                    "x at {i},{j}: {} vs {want_x}",
                    got[0]
                );
                assert!(
                    (f64::from(got[1]) - want_y).abs() < 1e-3,
                    "y at {i},{j}: {} vs {want_y}",
                    got[1]
                );
            }
        }
    }

    #[test]
    fn a_positive_angle_turns_the_picture_clockwise() {
        let img = ramp(60, 40);
        let out = apply_geometry(&img, &geo(|g| g.angle_deg = 15.0)).unwrap();
        let right = at(&out, out.width - 1, out.height / 2);
        let left = at(&out, 0, out.height / 2);
        // Clockwise: content on the right of the centre moves down, so the output's right edge
        // shows source rows above the centre, and the left edge rows below it.
        assert!(
            right[1] < 20.0 && left[1] > 20.0,
            "right {} left {}",
            right[1],
            left[1]
        );
    }

    #[test]
    fn there_are_no_empty_corners_and_the_aspect_ratio_is_kept() {
        let (w, h) = (120u32, 80u32);
        let img = ramp(w, h);
        for angle in [-30.0f32, -7.5, 3.0, 12.0, 44.0] {
            let out = apply_geometry(&img, &geo(|g| g.angle_deg = angle)).unwrap();
            for y in [0, out.height / 2, out.height - 1] {
                for x in [0, out.width / 2, out.width - 1] {
                    let p = at(&out, x, y);
                    assert!(
                        (0.0..=w as f32).contains(&p[0]) && (0.0..=h as f32).contains(&p[1]),
                        "{angle}: {p:?}"
                    );
                }
            }
            let ratio = f64::from(out.width) / f64::from(out.height);
            assert!(
                (ratio - 1.5).abs() < 0.1,
                "{angle}: {}x{}",
                out.width,
                out.height
            );
            assert!(out.width < w, "{angle}: straightening costs some frame");
        }
        let flat = apply_geometry(&img, &Geometry::default()).unwrap();
        assert_eq!((flat.width, flat.height), (w, h), "angle 0 loses nothing");
    }

    #[test]
    fn turns_and_flips_add_no_second_resample() {
        let img = numbered(40, 30);
        let straight = apply_geometry(&img, &geo(|g| g.angle_deg = 11.0)).unwrap();
        let combined = apply_geometry(
            &img,
            &geo(|g| {
                g.angle_deg = 11.0;
                g.rot90 = 1;
                g.flip_h = true;
            }),
        )
        .unwrap();
        let turned_after = apply_geometry(
            &straight,
            &geo(|g| {
                g.rot90 = 1;
                g.flip_h = true;
            }),
        )
        .unwrap();
        assert_eq!(
            combined, turned_after,
            "the same pixels as straightening first, then exact index moves"
        );
    }

    #[test]
    fn a_constant_image_stays_constant_when_straightened() {
        let img = ImageF32::new(30, 20, 3, vec![0.37; 30 * 20 * 3]).unwrap();
        let out = apply_geometry(&img, &geo(|g| g.angle_deg = -13.0)).unwrap();
        assert!(out.data.iter().all(|v| (v - 0.37).abs() < 1e-6));
    }

    #[test]
    fn other_channel_counts_work_too() {
        let ir = ImageF32::new(10, 6, 1, (0..60).map(|v| v as f32).collect()).unwrap();
        let out = apply_geometry(&ir, &geo(|g| g.rot90 = 1)).unwrap();
        assert_eq!((out.width, out.height, out.channels), (6, 10, 1));
        let rotated = apply_geometry(&ir, &geo(|g| g.angle_deg = 5.0)).unwrap();
        assert_eq!(rotated.channels, 1);
    }

    /// 21 megapixels (5616 x 3744) straightened; run with
    /// `cargo test -p gtp-edit --release -- --ignored --nocapture` to see the time.
    #[test]
    #[ignore = "timing run, not a check"]
    fn time_a_21_megapixel_straighten() {
        let (w, h) = (5616u32, 3744u32);
        let data: Vec<f32> = (0..w as usize * h as usize * 3)
            .map(|i| (i % 1000) as f32 / 1000.0)
            .collect();
        let img = ImageF32::new(w, h, 3, data).unwrap();
        let start = std::time::Instant::now();
        let out = apply_geometry(&img, &geo(|g| g.angle_deg = 1.5)).unwrap();
        eprintln!(
            "21 MP straighten -> {}x{}: {:?}",
            out.width,
            out.height,
            start.elapsed()
        );
    }

    proptest! {
        /// `output_size` always agrees with what `apply_geometry` produces.
        #[test]
        fn output_size_matches_the_result(
            w in 1u32..40, h in 1u32..40,
            cx in 0f32..=1.0, cy in 0f32..=1.0, cw in 0f32..=1.0, ch in 0f32..=1.0,
            angle in -45f32..=45.0, rot in -5i32..=5, fh: bool, fv: bool,
        ) {
            let g = Geometry { crop: crop(cx, cy, cw, ch), angle_deg: angle, rot90: rot, flip_h: fh, flip_v: fv };
            let img = ImageF32::zeros(w, h, 3).unwrap();
            match (output_size(w, h, &g), apply_geometry(&img, &g)) {
                (Ok(size), Ok(out)) => prop_assert_eq!(size, (out.width, out.height)),
                (Err(a), Err(b)) => prop_assert_eq!(a.code, b.code),
                (a, b) => prop_assert!(false, "disagree: {:?} vs {:?}", a.map(|_| ()), b.map(|_| ())),
            }
        }

        #[test]
        fn the_output_is_finite_and_not_negative(
            data in proptest::collection::vec(proptest::num::f32::NORMAL | proptest::num::f32::ZERO, 12 * 9 * 3),
            angle in -45f32..=45.0, rot in 0i32..4,
        ) {
            let img = ImageF32::new(12, 9, 3, data).unwrap();
            let g = Geometry { angle_deg: angle, rot90: rot, ..Geometry::default() };
            let out = apply_geometry(&img, &g).unwrap();
            prop_assert!(out.data.iter().all(|v| !v.is_nan() && *v >= 0.0));
        }
    }
}
