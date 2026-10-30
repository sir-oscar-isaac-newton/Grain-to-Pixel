//! Frame-area detection (docs/specification.md section 4.2, "Frame detection").
//!
//! The input is a scan divided by the film base ([`crate::normalize`]), so unexposed rebate is
//! about 1.0 and picture pixels are below it. A pixel counts as rebate when its luminance is
//! within [`REBATE_TOLERANCE`] of 1.0. Looking for the biggest brightness step instead would
//! fail on shadows (a thin-negative shadow is barely darker than the rebate) and on pictures with
//! hard steps inside them.
//!
//! Per axis, a median luminance profile is computed (the median of every column, and of every
//! row), and a rebate band is a run of rebate-level positions in the first or last quarter of the
//! axis. The frame edge is the inner end of each band; a side without a band keeps the full
//! extent, so rebate on one to four sides works. The confidence is the share of the outside area
//! that really is rebate times the share of the inside area that is not.

use gtp_core::recipe::Crop;
use gtp_core::{AppError, ErrorCode, ImageF32};

use crate::film_base::median;

/// How far from 1.0 a luminance may be to count as unexposed film.
pub const REBATE_TOLERANCE: f32 = 0.06;
/// Below this confidence [`detect_frame`] returns the full frame.
pub const MIN_CONFIDENCE: f32 = 0.5;
/// A profile uses at most this many sampled lines.
const MAX_PROFILE_LINES: usize = 512;
/// A rebate band must start and end within this fraction of the axis from an edge.
const SEARCH_FRACTION: f32 = 0.25;
/// The frame must span at least this fraction of each axis.
const MIN_FRAME_FRACTION: f32 = 0.2;

/// The detected frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameDetection {
    /// Crop rectangle as fractions of the scan (the Recipe's `geometry.crop`). The full frame
    /// when `confidence` is below [`MIN_CONFIDENCE`].
    pub rect: Crop,
    /// 0 to 1; how much the rebate and the picture look like what they should.
    pub confidence: f32,
}

fn is_rebate(value: f32) -> bool {
    (value - 1.0).abs() <= REBATE_TOLERANCE
}

/// Mean of the three channels of the pixel; a sample that is not finite counts as 0.
fn luminance(image: &ImageF32, x: usize, y: usize) -> f32 {
    let at = (y * image.width as usize + x) * 3;
    let sum: f32 = image.data.get(at..at + 3).map_or(0.0, |s| {
        s.iter().map(|v| if v.is_finite() { *v } else { 0.0 }).sum()
    });
    sum / 3.0
}

fn sample_step(len: usize) -> usize {
    len.div_ceil(MAX_PROFILE_LINES).max(1)
}

/// Median luminance of every column (`columns`) or row of the image, from sampled lines.
fn profile(image: &ImageF32, columns: bool) -> Vec<f32> {
    let (w, h) = (image.width as usize, image.height as usize);
    let (length, across) = if columns { (w, h) } else { (h, w) };
    let step = sample_step(across);
    let mut values = Vec::with_capacity(across / step + 1);
    (0..length)
        .map(|i| {
            values.clear();
            values.extend((0..across).step_by(step).map(|j| {
                if columns {
                    luminance(image, i, j)
                } else {
                    luminance(image, j, i)
                }
            }));
            median(&mut values).unwrap_or(0.0)
        })
        .collect()
}

/// Inner ends of the rebate bands of a profile: where the picture starts and where it ends.
/// `None` for a side without a band.
fn rebate_edges(profile: &[f32]) -> (Option<usize>, Option<usize>) {
    let n = profile.len();
    let window = ((n as f32 * SEARCH_FRACTION) as usize).max(1);
    let flags: Vec<bool> = profile.iter().map(|v| is_rebate(*v)).collect();

    let start = flags[..window.min(n)]
        .iter()
        .position(|f| *f)
        .and_then(|first| {
            let end = first + flags[first..].iter().take_while(|f| **f).count();
            (end <= window.min(n) && end < n).then_some(end)
        });
    let end = flags[n.saturating_sub(window)..]
        .iter()
        .rposition(|f| *f)
        .map(|last| n.saturating_sub(window) + last)
        .and_then(|last| {
            let begin = last + 1 - flags[..=last].iter().rev().take_while(|f| **f).count();
            (begin >= n.saturating_sub(window) && begin > 0).then_some(begin)
        });
    (start, end)
}

/// Lower and upper edge of one axis, if the edges are plausible; `found` says whether any band
/// was seen at all.
struct Axis {
    lo: usize,
    hi: usize,
    found: bool,
}

fn axis(profile: &[f32]) -> Option<Axis> {
    let n = profile.len();
    let (start, end) = rebate_edges(profile);
    let (lo, hi) = (start.unwrap_or(0), end.unwrap_or(n));
    let spans_enough = hi > lo && (hi - lo) as f32 >= MIN_FRAME_FRACTION * n as f32;
    spans_enough.then_some(Axis {
        lo,
        hi,
        found: start.is_some() || end.is_some(),
    })
}

fn full_frame(confidence: f32) -> FrameDetection {
    FrameDetection {
        rect: Crop::default(),
        confidence,
    }
}

/// Finds the exposed frame in a base-normalized scan.
///
/// Returns the full frame with a low confidence when no clear rebate is found: a blank or noisy
/// image, a flat dark image, or a scan cropped so tightly that it has no rebate.
pub fn detect_frame(image: &ImageF32) -> Result<FrameDetection, AppError> {
    if image.channels != 3 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Frame detection needs a 3-channel image.",
        )
        .with_details(format!("{} channels", image.channels)));
    }
    let (w, h) = (image.width as usize, image.height as usize);
    let (Some(x), Some(y)) = (axis(&profile(image, true)), axis(&profile(image, false))) else {
        return Ok(full_frame(0.0));
    };
    if !x.found && !y.found {
        return Ok(full_frame(0.0));
    }

    // Judge the rectangle on a grid of pixels: is the outside rebate, is the inside picture?
    let (step_x, step_y) = (sample_step(w), sample_step(h));
    let (mut outside, mut outside_rebate, mut inside, mut inside_picture) =
        (0u64, 0u64, 0u64, 0u64);
    for row in (0..h).step_by(step_y) {
        for col in (0..w).step_by(step_x) {
            let rebate = is_rebate(luminance(image, col, row));
            if (x.lo..x.hi).contains(&col) && (y.lo..y.hi).contains(&row) {
                inside += 1;
                inside_picture += u64::from(!rebate);
            } else {
                outside += 1;
                outside_rebate += u64::from(rebate);
            }
        }
    }
    let share = |part: u64, whole: u64| {
        if whole == 0 {
            0.0
        } else {
            part as f32 / whole as f32
        }
    };
    let confidence =
        (share(outside_rebate, outside) * share(inside_picture, inside)).clamp(0.0, 1.0);
    if confidence < MIN_CONFIDENCE {
        return Ok(full_frame(confidence));
    }

    let (wf, hf) = (w as f32, h as f32);
    Ok(FrameDetection {
        rect: Crop {
            x: x.lo as f32 / wf,
            y: y.lo as f32 / hf,
            w: (x.hi - x.lo) as f32 / wf,
            h: (y.hi - y.lo) as f32 / hf,
        },
        confidence,
    })
}

#[cfg(test)]
mod tests {
    use gtp_core::testing::synthetic::{self, Params, Pattern, Synthetic};
    use gtp_core::Recipe;
    use proptest::collection::vec;
    use proptest::num::f32::ANY;
    use proptest::prelude::*;

    use super::*;
    use crate::normalize;

    fn normalized(s: &Synthetic) -> ImageF32 {
        let mut image = s.image.clone();
        normalize(&mut image, s.truth.base).unwrap();
        image
    }

    /// The truth as fractions: left, top, right and bottom edge.
    fn truth_edges(s: &Synthetic) -> [f32; 4] {
        let (w, h) = (s.image.width as f32, s.image.height as f32);
        let f = s.truth.frame;
        [
            f.x as f32 / w,
            f.y as f32 / h,
            (f.x + f.w) as f32 / w,
            (f.y + f.h) as f32 / h,
        ]
    }

    fn found_edges(rect: Crop) -> [f32; 4] {
        [rect.x, rect.y, rect.x + rect.w, rect.y + rect.h]
    }

    fn assert_matches(s: &Synthetic, detection: &FrameDetection, context: &str) {
        let (want, got) = (truth_edges(s), found_edges(detection.rect));
        for (side, (g, w)) in ["left", "top", "right", "bottom"]
            .iter()
            .zip(got.iter().zip(want))
        {
            assert!((g - w).abs() <= 0.005, "{context}: {side} edge {g} vs {w}");
        }
    }

    #[test]
    fn the_frame_is_found_within_half_a_percent_for_every_layout() {
        let rebates = [
            [16; 4],
            [16, 0, 16, 0],
            [0, 16, 0, 16],
            [24, 8, 4, 16],
            [20, 0, 0, 0],
            [0, 0, 0, 12],
        ];
        for pattern in [Pattern::Wedge, Pattern::Gradient, Pattern::Blobs] {
            for rebate in rebates {
                for (width, height) in [(200, 150), (96, 64)] {
                    for noise in [0.0, 0.02] {
                        let rebate = if width == 96 {
                            rebate.map(|r| r.min(8))
                        } else {
                            rebate
                        };
                        let s = synthetic::negative(
                            9,
                            &Params {
                                width,
                                height,
                                rebate,
                                pattern,
                                noise,
                                ..Params::default()
                            },
                        )
                        .unwrap();
                        let detection = detect_frame(&normalized(&s)).unwrap();
                        let context =
                            format!("{pattern:?} {rebate:?} {width}x{height} noise {noise}");
                        assert!(
                            detection.confidence > 0.8,
                            "{context}: confidence {}",
                            detection.confidence
                        );
                        assert_matches(&s, &detection, &context);
                    }
                }
            }
        }
    }

    #[test]
    fn a_blank_scan_returns_the_full_frame_with_low_confidence() {
        let blank = ImageF32::new(60, 40, 3, vec![1.0; 60 * 40 * 3]).unwrap();
        let detection = detect_frame(&blank).unwrap();
        assert_eq!(detection.rect, Crop::default());
        assert!(detection.confidence < 0.2);
    }

    #[test]
    fn a_dark_flat_scan_returns_the_full_frame() {
        let dark = ImageF32::new(60, 40, 3, vec![0.3; 60 * 40 * 3]).unwrap();
        let detection = detect_frame(&dark).unwrap();
        assert_eq!(detection.rect, Crop::default());
        assert_eq!(detection.confidence, 0.0);
    }

    #[test]
    fn random_noise_returns_the_full_frame_with_low_confidence() {
        // A fixed xorshift sequence in [0, 1.2): about 5% of pixels fall within the rebate band.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 40) as f32 / 16_777_216.0 * 1.2
        };
        let data: Vec<f32> = (0..120 * 90 * 3).map(|_| next()).collect();
        let noise = ImageF32::new(120, 90, 3, data).unwrap();
        let detection = detect_frame(&noise).unwrap();
        assert_eq!(detection.rect, Crop::default());
        assert!(detection.confidence < 0.3, "{}", detection.confidence);
    }

    #[test]
    fn a_scan_without_any_rebate_is_not_trusted() {
        let s = synthetic::negative(
            3,
            &Params {
                width: 200,
                height: 150,
                rebate: [0; 4],
                ..Params::default()
            },
        )
        .unwrap();
        let detection = detect_frame(&normalized(&s)).unwrap();
        assert_eq!(detection.rect, Crop::default());
        assert!(detection.confidence < MIN_CONFIDENCE);
    }

    #[test]
    fn dust_in_the_picture_does_not_move_the_edges() {
        let s = synthetic::negative(
            5,
            &Params {
                width: 200,
                height: 150,
                rebate: [16; 4],
                dust_specks: 40,
                with_ir: true,
                ..Params::default()
            },
        )
        .unwrap();
        let detection = detect_frame(&normalized(&s)).unwrap();
        assert_matches(&s, &detection, "dust");
    }

    #[test]
    fn the_result_can_be_stored_in_a_recipe_unchanged() {
        let s = synthetic::negative(
            6,
            &Params {
                width: 200,
                height: 150,
                rebate: [24, 8, 4, 16],
                ..Params::default()
            },
        )
        .unwrap();
        let rect = detect_frame(&normalized(&s)).unwrap().rect;
        let mut recipe = Recipe::default();
        recipe.geometry.crop = rect;
        let clamped = recipe.clone().clamped().geometry.crop;
        for (a, b) in found_edges(rect).iter().zip(found_edges(clamped)) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn a_non_rgb_image_is_invalid_input() {
        let gray = ImageF32::zeros(10, 10, 1).unwrap();
        assert_eq!(
            detect_frame(&gray).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn tiny_images_do_not_panic() {
        for (w, h) in [(1, 1), (2, 1), (1, 5), (3, 3)] {
            let image = ImageF32::new(w, h, 3, vec![1.0; (w * h * 3) as usize]).unwrap();
            let detection = detect_frame(&image).unwrap();
            assert_eq!(detection.rect, Crop::default());
        }
    }

    proptest! {
        #[test]
        fn the_rectangle_is_always_valid(w in 1u32..24, h in 1u32..24, data in vec(ANY, 24 * 24 * 3)) {
            let count = (w * h * 3) as usize;
            let image = ImageF32::new(w, h, 3, data[..count].to_vec()).unwrap();
            let d = detect_frame(&image).unwrap();
            prop_assert!((0.0..=1.0).contains(&d.confidence));
            let r = d.rect;
            prop_assert!(r.x >= 0.0 && r.y >= 0.0 && r.w > 0.0 && r.h > 0.0);
            prop_assert!(r.x + r.w <= 1.0 + 1e-6 && r.y + r.h <= 1.0 + 1e-6);
        }
    }
}
