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
use rayon::prelude::*;

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
    /// The frame's width and height as fractions of the image, when rebate on all four sides
    /// measured them; the roll's frame size is calibrated from these.
    pub measured_size: Option<[f32; 2]>,
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
    (0..length)
        .into_par_iter()
        .map(|i| {
            let mut values = Vec::with_capacity(across / step + 1);
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
    /// Whether both edges were found.
    both: bool,
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
        both: start.is_some() && end.is_some(),
    })
}

fn full_frame(confidence: f32) -> FrameDetection {
    FrameDetection {
        rect: Crop::default(),
        confidence,
        measured_size: None,
    }
}

/// What is known about the size of a frame, to place a window of that size on a camera capture
/// (see [`detect_frame_with`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameHint {
    /// Long side over short side of the frame (about 1.34 for 645).
    pub aspect: f32,
    /// The frame's width and height as fractions of the image, when known (the roll's).
    pub size: Option<[f32; 2]>,
}

/// Finds the exposed frame in a base-normalized scan.
///
/// Returns the full frame with a low confidence when no clear rebate is found: a blank or noisy
/// image, a flat dark image, or a scan cropped so tightly that it has no rebate.
pub fn detect_frame(image: &ImageF32) -> Result<FrameDetection, AppError> {
    detect_frame_with(image, None)
}

/// Like [`detect_frame`], and when the rebate is not enough and a `hint` is given, treats near-black
/// pixels as outside the film and slides a window of the frame's size to where it fits the picture
/// best. Without a hint this is exactly [`detect_frame`].
pub fn detect_frame_with(
    image: &ImageF32,
    hint: Option<FrameHint>,
) -> Result<FrameDetection, AppError> {
    if image.channels != 3 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Frame detection needs a 3-channel image.",
        )
        .with_details(format!("{} channels", image.channels)));
    }
    let by_rebate = detect_by_rebate(image);
    let Some(hint) = hint else {
        return Ok(by_rebate);
    };
    if by_rebate.confidence >= MIN_CONFIDENCE {
        return Ok(by_rebate);
    }
    let windowed = detect_by_window(image, hint);
    Ok(if windowed.confidence > by_rebate.confidence {
        windowed
    } else {
        by_rebate
    })
}

/// Without the roll's frame size, the window is this share of the largest one of the format's aspect
/// that fits the film area: a frame is a little smaller than the opening it is shot through, and the
/// strips of film edge at the border of the opening are not picture. Measured on the author's 645
/// roll against Trichroma Lab's crops (0.97 of the opening on both axes).
const PRIOR_FIT: f32 = 0.97;

/// A profile value at or below this is the black of the carrier, not film.
pub const BLACK_LEVEL: f32 = 0.02;

fn is_black(value: f32) -> bool {
    value <= BLACK_LEVEL
}

/// A picture pixel: neither carrier black nor unexposed film.
fn is_picture(value: f32) -> bool {
    !is_black(value) && !is_rebate(value)
}

/// Median luminance of every column (`columns`) or row, over the sampled lines in `across`.
fn profile_in(image: &ImageF32, columns: bool, across: std::ops::Range<usize>) -> Vec<f32> {
    let (w, h) = (image.width as usize, image.height as usize);
    let length = if columns { w } else { h };
    let step = sample_step(across.len());
    (0..length)
        .into_par_iter()
        .map(|i| {
            let mut values: Vec<f32> = across
                .clone()
                .step_by(step)
                .map(|j| {
                    if columns {
                        luminance(image, i, j)
                    } else {
                        luminance(image, j, i)
                    }
                })
                .collect();
            median(&mut values).unwrap_or(0.0)
        })
        .collect()
}

/// The part of a profile between its black ends.
fn film_range(profile: &[f32]) -> std::ops::Range<usize> {
    let lo = profile.iter().take_while(|v| is_black(**v)).count();
    let hi = profile.len() - profile.iter().rev().take_while(|v| is_black(**v)).count();
    if hi > lo {
        lo..hi
    } else {
        0..0
    }
}

/// The start of the window of `len` positions that covers the most picture in `profile`, within
/// `range`; the one nearest the middle wins a tie. `None` when it does not fit.
fn best_window(profile: &[f32], range: &std::ops::Range<usize>, len: usize) -> Option<usize> {
    if len == 0 || range.len() < len {
        return None;
    }
    let mut prefix = vec![0i64; range.len() + 1];
    for (k, v) in profile[range.clone()].iter().enumerate() {
        prefix[k + 1] = prefix[k] + if is_picture(*v) { 1 } else { -1 };
    }
    let middle = (range.len() - len) as f32 / 2.0;
    (0..=range.len() - len)
        .max_by(|a, b| {
            let (sa, sb) = (prefix[a + len] - prefix[*a], prefix[b + len] - prefix[*b]);
            let (da, db) = ((*a as f32 - middle).abs(), (*b as f32 - middle).abs());
            sa.cmp(&sb).then(db.total_cmp(&da))
        })
        .map(|start| range.start + start)
}

/// The window found for one orientation.
fn window_for(
    image: &ImageF32,
    film: &(std::ops::Range<usize>, std::ops::Range<usize>),
    (win_w, win_h): (usize, usize),
) -> Option<FrameDetection> {
    let (w, h) = (image.width as usize, image.height as usize);
    let cols = profile_in(image, true, film.1.clone());
    let x0 = best_window(&cols, &film.0, win_w)?;
    let rows = profile_in(image, false, x0..x0 + win_w);
    let y0 = best_window(&rows, &film.1, win_h)?;

    let (step_x, step_y) = (sample_step(w), sample_step(h));
    let (mut outside, mut outside_flat, mut inside, mut inside_picture) = (0u64, 0u64, 0u64, 0u64);
    for row in film.1.clone().step_by(step_y) {
        for col in film.0.clone().step_by(step_x) {
            let picture = is_picture(luminance(image, col, row));
            if (x0..x0 + win_w).contains(&col) && (y0..y0 + win_h).contains(&row) {
                inside += 1;
                inside_picture += u64::from(picture);
            } else {
                outside += 1;
                outside_flat += u64::from(!picture);
            }
        }
    }
    let share = |part: u64, whole: u64| {
        if whole == 0 {
            1.0
        } else {
            part as f32 / whole as f32
        }
    };
    // A neighbouring frame may be in view beside the window, so the outside only counts for half.
    let confidence = (share(inside_picture, inside) * (0.5 + 0.5 * share(outside_flat, outside)))
        .clamp(0.0, 1.0);
    let (wf, hf) = (w as f32, h as f32);
    Some(FrameDetection {
        rect: Crop {
            x: x0 as f32 / wf,
            y: y0 as f32 / hf,
            w: win_w as f32 / wf,
            h: win_h as f32 / hf,
        },
        confidence,
        measured_size: None,
    })
}

/// Places a window of the frame's size on the film part of the image (everything but the black
/// carrier). With the roll's size the window is that size; otherwise it is the largest one of the
/// format's aspect that fits, tried in both orientations, and the better fit wins.
fn detect_by_window(image: &ImageF32, hint: FrameHint) -> FrameDetection {
    let (w, h) = (image.width as usize, image.height as usize);
    let film_x = film_range(&profile_in(image, true, 0..h));
    if film_x.is_empty() {
        return full_frame(0.0);
    }
    let film_y = film_range(&profile_in(image, false, film_x.clone()));
    if film_y.is_empty() {
        return full_frame(0.0);
    }
    let film = (film_x, film_y);
    let (area_w, area_h) = (film.0.len() as f32, film.1.len() as f32);

    let sizes: Vec<(usize, usize)> = match hint.size {
        Some([fw, fh]) => {
            let fit = |len: f32, area: f32| (len.round() as usize).clamp(1, area as usize);
            vec![(fit(fw * w as f32, area_w), fit(fh * h as f32, area_h))]
        }
        None if hint.aspect.is_finite() && hint.aspect >= 1.0 => {
            let landscape_h = area_h.min(area_w / hint.aspect) * PRIOR_FIT;
            let portrait_w = area_w.min(area_h / hint.aspect) * PRIOR_FIT;
            vec![
                (
                    (landscape_h * hint.aspect).round() as usize,
                    landscape_h.round() as usize,
                ),
                (
                    portrait_w.round() as usize,
                    (portrait_w * hint.aspect).round() as usize,
                ),
            ]
        }
        None => Vec::new(),
    };
    sizes
        .into_iter()
        .filter_map(|size| window_for(image, &film, size))
        .max_by(|a, b| a.confidence.total_cmp(&b.confidence))
        .unwrap_or_else(|| full_frame(0.0))
}

fn detect_by_rebate(image: &ImageF32) -> FrameDetection {
    let (w, h) = (image.width as usize, image.height as usize);
    let (Some(x), Some(y)) = (axis(&profile(image, true)), axis(&profile(image, false))) else {
        return full_frame(0.0);
    };
    if !x.found && !y.found {
        return full_frame(0.0);
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
        return full_frame(confidence);
    }

    let (wf, hf) = (w as f32, h as f32);
    let rect = Crop {
        x: x.lo as f32 / wf,
        y: y.lo as f32 / hf,
        w: (x.hi - x.lo) as f32 / wf,
        h: (y.hi - y.lo) as f32 / hf,
    };
    // Rebate on all four sides measured the whole frame, so its size can teach the roll's.
    let measured = x.both && y.both;
    FrameDetection {
        rect,
        confidence,
        measured_size: measured.then_some([rect.w, rect.h]),
    }
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

    /// A 300 x 200 capture: opaque black left of 40 and right of 260, unexposed film from 40 to
    /// `picture_x`, then a picture of `picture_w` columns, then unexposed film again.
    fn camera_capture(picture_x: usize, picture_w: usize) -> ImageF32 {
        let (w, h) = (300usize, 200usize);
        let mut data = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let v = if !(40..260).contains(&x) {
                    0.003
                } else if (picture_x..picture_x + picture_w).contains(&x) {
                    0.3 + 0.4 * ((x * 7 + y * 3) % 10) as f32 / 10.0
                } else {
                    1.0
                };
                data.extend([v; 3]);
            }
        }
        ImageF32::new(w as u32, h as u32, 3, data).unwrap()
    }

    const HINT_645: FrameHint = FrameHint {
        aspect: 1.34,
        size: None,
    };

    #[test]
    fn a_window_of_the_formats_size_finds_a_frame_between_black_margins() {
        // One rebate edge only, too wide to count as a band, so the rebate search gives up.
        let image = camera_capture(111, 149);
        assert!(detect_frame(&image).unwrap().confidence < MIN_CONFIDENCE);
        assert!(detect_frame_with(&image, None).unwrap().confidence < MIN_CONFIDENCE);

        let found = detect_frame_with(&image, Some(HINT_645)).unwrap();
        assert!(found.confidence >= MIN_CONFIDENCE, "{}", found.confidence);
        // Without the roll's size the window is PRIOR_FIT of the largest that fits, so it sits
        // inside the picture, which spans columns 111 to 260.
        let right = found.rect.x + found.rect.w;
        assert!(
            found.rect.x >= 111.0 / 300.0 - 0.005 && right <= 260.0 / 300.0 + 0.005,
            "{:?}",
            found.rect
        );
        assert!(
            (found.rect.w - 149.0 * PRIOR_FIT / 300.0).abs() < 0.01,
            "{:?}",
            found.rect
        );
        assert!(found.rect.h > 0.9, "{:?}", found.rect);
        assert!(found.measured_size.is_none());
    }

    #[test]
    fn the_rolls_size_places_the_window_and_the_picture_can_sit_anywhere() {
        let hint = FrameHint {
            aspect: 1.34,
            size: Some([149.0 / 300.0, 1.0]),
        };
        for picture_x in [40, 77, 111] {
            let image = camera_capture(picture_x, 149);
            let found = detect_frame_with(&image, Some(hint)).unwrap();
            assert!(found.confidence >= MIN_CONFIDENCE, "{picture_x}");
            assert!(
                (found.rect.x - picture_x as f32 / 300.0).abs() < 0.01,
                "{picture_x}: {:?}",
                found.rect
            );
        }
    }

    #[test]
    fn a_confident_rebate_detection_is_kept_and_measures_the_frame() {
        let s = synthetic::negative(
            9,
            &Params {
                width: 200,
                height: 150,
                rebate: [16; 4],
                ..Params::default()
            },
        )
        .unwrap();
        let image = normalized(&s);
        let plain = detect_frame(&image).unwrap();
        let hinted = detect_frame_with(&image, Some(HINT_645)).unwrap();
        assert_eq!(plain, hinted);
        let size = plain.measured_size.unwrap();
        assert!((size[0] - plain.rect.w).abs() < 1e-6 && (size[1] - plain.rect.h).abs() < 1e-6);
    }

    #[test]
    fn nothing_but_black_gives_the_full_frame_even_with_a_hint() {
        let black = ImageF32::new(60, 40, 3, vec![0.0; 60 * 40 * 3]).unwrap();
        let found = detect_frame_with(&black, Some(HINT_645)).unwrap();
        assert_eq!(found.rect, Crop::default());
        assert_eq!(found.confidence, 0.0);
        let bad = FrameHint {
            aspect: f32::NAN,
            size: None,
        };
        let image = camera_capture(111, 149);
        assert_eq!(
            detect_frame_with(&image, Some(bad)).unwrap().rect,
            Crop::default()
        );
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
