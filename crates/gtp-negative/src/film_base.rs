//! Film-base measurement (docs/specification.md section 4.2, "Film base").
//!
//! The film base is the colour of unexposed film: the brightest thing on a negative. It is read
//! from the rebate, either where the user points ([`sample_film_base`]) or automatically from the
//! border of the scan ([`auto_film_base`]), and combined over a roll with [`roll_base`]. Every
//! measurement uses medians, so a few specks of dust or hot pixels cannot move the result.

use gtp_core::{AppError, ErrorCode, ImageF32};

/// Width of the border strips used by [`auto_film_base`], as a fraction of the shorter side.
const STRIP_FRACTION: f32 = 0.02;
/// The interior is sampled on a grid of at most about this many pixels.
const MAX_INTERIOR_SAMPLES: u64 = 100_000;
/// A strip whose median absolute deviation reaches this fraction of its median counts as not
/// uniform at all.
const SPREAD_LIMIT: f32 = 0.25;
/// A border this much brighter than the interior counts as fully separated from the picture.
const FULL_SEPARATION: f32 = 0.5;

/// A rectangle of pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// An automatic film-base measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BaseEstimate {
    /// Linear scan value of unexposed film, per channel.
    pub rgb: [f32; 3],
    /// 0 to 1: how much to trust `rgb`. Low means the border does not look like film rebate
    /// (no clear unexposed strip), and the user should sample the base by hand.
    pub confidence: f32,
}

fn invalid(message: &str, details: String) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message).with_details(details)
}

fn require_rgb(image: &ImageF32) -> Result<(), AppError> {
    if image.channels == 3 {
        Ok(())
    } else {
        Err(invalid(
            "Film-base measurement needs a 3-channel image.",
            format!("{} channels", image.channels),
        ))
    }
}

fn check_rect(image: &ImageF32, rect: Rect) -> Result<(), AppError> {
    let inside = u64::from(rect.x) + u64::from(rect.w) <= u64::from(image.width)
        && u64::from(rect.y) + u64::from(rect.h) <= u64::from(image.height);
    if rect.w == 0 || rect.h == 0 || !inside {
        return Err(invalid(
            "That area is outside the image.",
            format!("{rect:?} in {}x{}", image.width, image.height),
        ));
    }
    Ok(())
}

/// Median of the values (the mean of the two middle ones for an even count); `None` if empty.
/// Reorders the slice. The values must not be NaN.
pub(crate) fn median(values: &mut [f32]) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    let len = values.len();
    let (lower, upper, _) = values.select_nth_unstable_by(len / 2, f32::total_cmp);
    let upper = *upper;
    if len % 2 == 1 {
        return Some(upper);
    }
    let below = lower.iter().copied().fold(f32::MIN, f32::max);
    Some((below + upper) / 2.0)
}

/// Finite samples of each channel from `rect`, taking every `step`-th pixel in both directions.
fn channel_samples(image: &ImageF32, rect: Rect, step: u32) -> [Vec<f32>; 3] {
    let mut samples: [Vec<f32>; 3] = Default::default();
    let width = image.width as usize;
    let step = step.max(1) as usize;
    for y in (rect.y..rect.y + rect.h).step_by(step) {
        for x in (rect.x..rect.x + rect.w).step_by(step) {
            let at = (y as usize * width + x as usize) * 3;
            if let Some(&[r, g, b]) = image
                .data
                .get(at..at + 3)
                .and_then(|s| <&[f32; 3]>::try_from(s).ok())
            {
                for (channel, value) in samples.iter_mut().zip([r, g, b]) {
                    if value.is_finite() {
                        channel.push(value);
                    }
                }
            }
        }
    }
    samples
}

/// Per-channel median and median absolute deviation, or `None` if a channel has no samples.
fn medians_and_spread(mut samples: [Vec<f32>; 3]) -> Option<([f32; 3], [f32; 3])> {
    let mut medians = [0.0; 3];
    let mut spreads = [0.0; 3];
    for (c, channel) in samples.iter_mut().enumerate() {
        let m = median(channel)?;
        let mut deviations: Vec<f32> = channel.iter().map(|v| (v - m).abs()).collect();
        medians[c] = m;
        spreads[c] = median(&mut deviations)?;
    }
    Some((medians, spreads))
}

fn is_usable_base(rgb: [f32; 3]) -> bool {
    rgb.iter().all(|v| v.is_finite() && *v > 0.0)
}

fn mean(rgb: [f32; 3]) -> f32 {
    (rgb[0] + rgb[1] + rgb[2]) / 3.0
}

/// The film base from a user-chosen area of the rebate: the per-channel median of the area.
/// A click should pass a small square around the point, a dragged box its own rectangle.
pub fn sample_film_base(image: &ImageF32, region: Rect) -> Result<[f32; 3], AppError> {
    require_rgb(image)?;
    check_rect(image, region)?;
    let (rgb, _) = medians_and_spread(channel_samples(image, region, 1))
        .ok_or_else(|| invalid("That area has no usable pixels.", format!("{region:?}")))?;
    if !is_usable_base(rgb) {
        return Err(invalid(
            "That area is too dark to be film base.",
            format!("median {rgb:?}"),
        ));
    }
    Ok(rgb)
}

/// Measures the film base from the border of the scan.
///
/// Takes a thin strip along each of the four edges, computes its per-channel median and keeps the
/// brightest strip, because unexposed film is brighter than any picture content. The confidence
/// combines how uniform that strip is with how much brighter it is than the interior of the
/// picture. A scan cropped so tightly that it has no rebate gets a low confidence.
pub fn auto_film_base(image: &ImageF32) -> Result<BaseEstimate, AppError> {
    require_rgb(image)?;
    let (width, height) = (image.width, image.height);
    let shorter = width.min(height);
    let strip = ((shorter as f32 * STRIP_FRACTION).round() as u32).clamp(1, shorter);

    let strips = [
        Rect {
            x: 0,
            y: 0,
            w: strip,
            h: height,
        },
        Rect {
            x: width - strip,
            y: 0,
            w: strip,
            h: height,
        },
        Rect {
            x: 0,
            y: 0,
            w: width,
            h: strip,
        },
        Rect {
            x: 0,
            y: height - strip,
            w: width,
            h: strip,
        },
    ];
    let best = strips
        .into_iter()
        .filter_map(|rect| medians_and_spread(channel_samples(image, rect, 1)))
        .filter(|(rgb, _)| is_usable_base(*rgb))
        .max_by(|a, b| mean(a.0).total_cmp(&mean(b.0)))
        .ok_or_else(|| {
            invalid(
                "No part of the border looks like film base.",
                format!("{width}x{height}"),
            )
        })?;
    let (rgb, spread) = best;

    let relative_spread = (0..3).map(|c| spread[c] / rgb[c]).fold(0.0, f32::max);
    let uniformity = 1.0 - (relative_spread / SPREAD_LIMIT).clamp(0.0, 1.0);

    let interior = if width > 2 * strip && height > 2 * strip {
        let (w, h) = (width - 2 * strip, height - 2 * strip);
        let step = ((f64::from(w) * f64::from(h) / MAX_INTERIOR_SAMPLES as f64)
            .sqrt()
            .ceil() as u32)
            .max(1);
        let rect = Rect {
            x: strip,
            y: strip,
            w,
            h,
        };
        medians_and_spread(channel_samples(image, rect, step)).map(|(m, _)| mean(m))
    } else {
        None
    };
    let separation = match interior {
        Some(inside) if inside > 0.0 => {
            ((mean(rgb) / inside - 1.0) / FULL_SEPARATION).clamp(0.0, 1.0)
        }
        Some(_) => 1.0,
        None => 0.0,
    };

    Ok(BaseEstimate {
        rgb,
        confidence: (uniformity * separation).clamp(0.0, 1.0),
    })
}

/// Combines the film bases measured on the frames of a roll: the per-channel median, so one
/// badly measured frame is ignored. Bases that are not finite and above 0 are skipped. With two
/// frames the result is their mean, so an outlier needs at least three frames to be rejected.
pub fn roll_base(bases: &[[f32; 3]]) -> Result<[f32; 3], AppError> {
    let mut channels: [Vec<f32>; 3] = Default::default();
    for base in bases.iter().filter(|b| is_usable_base(**b)) {
        for (channel, value) in channels.iter_mut().zip(base) {
            channel.push(*value);
        }
    }
    let mut rgb = [0.0; 3];
    for (out, channel) in rgb.iter_mut().zip(channels.iter_mut()) {
        *out = median(channel).ok_or_else(|| {
            invalid(
                "There is no usable film base to combine.",
                format!("{} bases given", bases.len()),
            )
        })?;
    }
    Ok(rgb)
}

#[cfg(test)]
mod tests {
    use gtp_core::testing::synthetic::{self, Params, Pattern, Synthetic};

    use super::*;

    fn scan(seed: u64, params: &Params) -> Synthetic {
        synthetic::negative(seed, params).unwrap()
    }

    fn assert_within(got: [f32; 3], want: [f32; 3], fraction: f32) {
        for c in 0..3 {
            let error = (got[c] - want[c]).abs() / want[c];
            assert!(
                error <= fraction,
                "channel {c}: {} vs {} ({:.2}% off)",
                got[c],
                want[c],
                error * 100.0
            );
        }
    }

    fn rebate_rect(s: &Synthetic) -> Rect {
        // The left rebate, one pixel away from the frame, a few pixels wide.
        Rect {
            x: 1,
            y: 2,
            w: s.truth.frame.x.saturating_sub(3).max(1),
            h: s.image.height - 4,
        }
    }

    #[test]
    fn the_auto_base_is_within_one_percent_for_every_pattern_rebate_and_noise_level() {
        let rebates = [
            [16; 4],
            [16, 0, 16, 0],
            [0, 16, 0, 16],
            [16, 16, 0, 0],
            [24, 8, 4, 16],
        ];
        for pattern in [Pattern::Wedge, Pattern::Gradient, Pattern::Blobs] {
            for rebate in rebates {
                for noise in [0.0, 0.02] {
                    let params = Params {
                        width: 200,
                        height: 150,
                        rebate,
                        pattern,
                        noise,
                        ..Params::default()
                    };
                    let s = scan(11, &params);
                    let estimate = auto_film_base(&s.image).unwrap();
                    assert_within(estimate.rgb, s.truth.base, 0.01);
                }
            }
        }
    }

    #[test]
    fn the_default_small_synthetic_image_also_works() {
        let s = scan(1, &Params::default());
        assert_within(auto_film_base(&s.image).unwrap().rgb, s.truth.base, 0.01);
    }

    #[test]
    fn confidence_is_high_with_a_rebate_and_low_without_one() {
        let with = scan(
            2,
            &Params {
                width: 200,
                height: 150,
                rebate: [16; 4],
                ..Params::default()
            },
        );
        assert!(auto_film_base(&with.image).unwrap().confidence > 0.9);

        let without = scan(
            2,
            &Params {
                width: 200,
                height: 150,
                rebate: [0; 4],
                ..Params::default()
            },
        );
        let low = auto_film_base(&without.image).unwrap().confidence;
        assert!(
            low < 0.5,
            "a scan with no rebate must not look trustworthy: {low}"
        );
    }

    #[test]
    fn a_flat_image_has_no_confidence() {
        let flat = ImageF32::new(40, 30, 3, vec![0.4; 40 * 30 * 3]).unwrap();
        let estimate = auto_film_base(&flat).unwrap();
        assert_eq!(estimate.confidence, 0.0);
        assert_within(estimate.rgb, [0.4; 3], 1e-6);
    }

    #[test]
    fn a_very_small_image_still_returns_an_estimate() {
        let tiny = ImageF32::new(1, 1, 3, vec![0.5, 0.4, 0.3]).unwrap();
        let estimate = auto_film_base(&tiny).unwrap();
        assert_eq!(estimate.rgb, [0.5, 0.4, 0.3]);
        assert_eq!(estimate.confidence, 0.0);
    }

    #[test]
    fn an_all_black_or_non_finite_border_is_an_error() {
        let black = ImageF32::zeros(20, 20, 3).unwrap();
        assert_eq!(
            auto_film_base(&black).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let nan = ImageF32::new(4, 4, 3, vec![f32::NAN; 48]).unwrap();
        assert_eq!(
            auto_film_base(&nan).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn sampling_the_rebate_gives_the_base_and_sampling_the_picture_does_not() {
        let s = scan(
            3,
            &Params {
                width: 200,
                height: 150,
                rebate: [20; 4],
                ..Params::default()
            },
        );
        let base = sample_film_base(&s.image, rebate_rect(&s)).unwrap();
        assert_within(base, s.truth.base, 0.005);

        let frame = s.truth.frame;
        let picture = Rect {
            x: frame.x + frame.w / 2,
            y: frame.y + frame.h / 2,
            w: 10,
            h: 10,
        };
        let wrong = sample_film_base(&s.image, picture).unwrap();
        assert!(
            wrong[0] < s.truth.base[0] * 0.9,
            "the picture is darker than the base"
        );
    }

    #[test]
    fn dust_and_hot_pixels_in_the_sample_area_do_not_move_the_median() {
        let mut s = scan(
            4,
            &Params {
                width: 200,
                height: 150,
                rebate: [20; 4],
                ..Params::default()
            },
        );
        let rect = rebate_rect(&s);
        let mut k = 0;
        for y in rect.y..rect.y + rect.h {
            for x in rect.x..rect.x + rect.w {
                k += 1;
                let at = (y as usize * 200 + x as usize) * 3;
                if k % 17 == 0 {
                    for v in &mut s.image.data[at..at + 3] {
                        *v *= 0.25; // dust
                    }
                } else if k % 101 == 0 {
                    for v in &mut s.image.data[at..at + 3] {
                        *v = 1.0e6; // hot pixel
                    }
                }
            }
        }
        assert_within(
            sample_film_base(&s.image, rect).unwrap(),
            s.truth.base,
            0.01,
        );
    }

    #[test]
    fn rectangles_outside_the_image_or_empty_are_rejected() {
        let img = ImageF32::new(10, 10, 3, vec![0.5; 300]).unwrap();
        for rect in [
            Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 5,
            },
            Rect {
                x: 0,
                y: 0,
                w: 5,
                h: 0,
            },
            Rect {
                x: 6,
                y: 0,
                w: 5,
                h: 5,
            },
            Rect {
                x: 0,
                y: 9,
                w: 5,
                h: 2,
            },
            Rect {
                x: u32::MAX,
                y: 0,
                w: u32::MAX,
                h: 1,
            },
        ] {
            let err = sample_film_base(&img, rect).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{rect:?}");
        }
        assert!(sample_film_base(
            &img,
            Rect {
                x: 5,
                y: 5,
                w: 5,
                h: 5
            }
        )
        .is_ok());
    }

    #[test]
    fn a_dark_area_and_a_non_rgb_image_are_rejected() {
        let dark = ImageF32::zeros(10, 10, 3).unwrap();
        let all = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        };
        assert_eq!(
            sample_film_base(&dark, all).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let gray = ImageF32::zeros(10, 10, 1).unwrap();
        assert_eq!(
            sample_film_base(&gray, all).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        assert_eq!(
            auto_film_base(&gray).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn the_roll_base_ignores_an_outlier_frame() {
        let truth = [0.80, 0.55, 0.35];
        let nudge = |f: f32| truth.map(|v| v * f);
        let bases = [
            nudge(1.00),
            nudge(1.01),
            nudge(0.99),
            [0.1, 0.9, 0.9],
            nudge(1.005),
        ];
        assert_within(roll_base(&bases).unwrap(), truth, 0.01);
    }

    #[test]
    fn the_roll_base_edge_cases() {
        let one = [0.8, 0.5, 0.3];
        assert_eq!(roll_base(&[one]).unwrap(), one);
        assert_within(
            roll_base(&[[0.8, 0.6, 0.4], [0.6, 0.4, 0.2]]).unwrap(),
            [0.7, 0.5, 0.3],
            1e-6,
        );
        assert_eq!(roll_base(&[]).unwrap_err().code, ErrorCode::InvalidInput);

        let junk = [
            [f32::NAN, 1.0, 1.0],
            [0.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
            [f32::INFINITY, 1.0, 1.0],
        ];
        assert_eq!(roll_base(&junk).unwrap_err().code, ErrorCode::InvalidInput);
        let mixed = [junk[0], one, junk[1], one, one];
        assert_eq!(
            roll_base(&mixed).unwrap(),
            one,
            "unusable bases are skipped"
        );
    }
}
