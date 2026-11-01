//! IR dust removal (docs/specification.md section 4.2, step 2): the infrared plane shows dust and
//! scratches as dark spots, because dyes pass infrared but dust does not. The stage finds those
//! spots, widens them a little and fills them in from their surroundings.
//!
//! ```text
//! clear  = the DUST_CLEAR_PERCENTILE of the infrared plane (what a clean pixel reads)
//! mask   = ir < clear * (1 - threshold)
//! mask   = mask dilated by grow_px (a square of side 2 * grow_px + 1)
//! fill   = masked pixels, from the outside in, each the weighted mean of its known neighbours
//! ```
//!
//! A higher `threshold` masks fewer pixels (only deeper shadows count) and a larger `grow_px` masks
//! more, so each is monotonic. The plane is read relative to its own clear level because gain and
//! film differ between scans. Pixels that are not masked are never touched. The fill works in
//! passes: in each, every masked pixel that has a known neighbour (8-neighbourhood, the four
//! straight ones weighing 1 and the diagonal ones [`DIAGONAL_WEIGHT`]) becomes their mean, and then
//! counts as known for the next pass, so the fill closes in from the edge of a spot to its centre.
//!
//! An infrared plane that masks more than [`MAX_MASKED_FRACTION`] of the picture is not showing
//! dust (a plane of the wrong kind, a scan of a dark film) and is ignored, so a bad plane cannot
//! wipe out the picture. The radius is in pixels of the image it is given and is not scaled.

use gtp_core::recipe::Dust;
use gtp_core::{AppError, ErrorCode, ImageF32};
use rayon::prelude::*;

use crate::{require_rgb, MAX_SAMPLE};

/// The share of the infrared plane, from the dark end, that is taken to be dust: a clean pixel
/// reads the value at this percentile or above.
pub const DUST_CLEAR_PERCENTILE: f32 = 0.9;
/// Weight of a diagonal neighbour in the fill, against 1 for a straight one.
pub const DIAGONAL_WEIGHT: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// A plane that masks more than this share of the picture is ignored.
pub const MAX_MASKED_FRACTION: f32 = 0.25;
/// Steps of the histogram the clear level is read from.
const BINS: usize = 4096;
/// Below this the plane is dark everywhere and says nothing about dust.
const MIN_CLEAR: f32 = 1e-4;

/// What the stage did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DustReport {
    /// Pixels in the mask, after growing it.
    pub masked: usize,
    /// False when the stage left the image alone: the plane was dark or masked too much.
    pub applied: bool,
}

/// The value at `DUST_CLEAR_PERCENTILE` of the plane, to the width of a histogram step.
fn clear_level(plane: &[f32]) -> f32 {
    let bin_of = |v: f32| ((v.clamp(0.0, 1.0) * (BINS - 1) as f32) as usize).min(BINS - 1);
    let mut histogram = vec![0usize; BINS];
    for &v in plane {
        histogram[bin_of(if v.is_finite() { v } else { 0.0 })] += 1;
    }
    let target = (plane.len() as f32 * DUST_CLEAR_PERCENTILE) as usize;
    let mut seen = 0;
    for (bin, count) in histogram.iter().enumerate() {
        seen += count;
        if seen > target {
            return bin as f32 / (BINS - 1) as f32;
        }
    }
    1.0
}

/// `mask` with every flagged pixel widened by `grow` pixels in each direction.
fn dilate(mask: &[bool], width: usize, grow: usize) -> Vec<bool> {
    if grow == 0 {
        return mask.to_vec();
    }
    let height = mask.len() / width;
    // Along the rows: a pixel is flagged when any pixel within `grow` of it is.
    let mut wide = vec![false; mask.len()];
    wide.par_chunks_mut(width)
        .zip(mask.par_chunks(width))
        .for_each(|(dst, src)| {
            let mut inside = src[..grow.min(width - 1) + 1]
                .iter()
                .filter(|&&m| m)
                .count();
            for (x, slot) in dst.iter_mut().enumerate() {
                *slot = inside > 0;
                if x + grow + 1 < width && src[x + grow + 1] {
                    inside += 1;
                }
                if x >= grow && src[x - grow] {
                    inside -= 1;
                }
            }
        });
    // Down the columns: the OR of the `2 * grow + 1` rows around each row.
    let mut out = vec![false; mask.len()];
    out.par_chunks_mut(width).enumerate().for_each(|(y, dst)| {
        let first = y.saturating_sub(grow);
        let last = (y + grow).min(height - 1);
        for row in first..=last {
            for (slot, &flag) in dst.iter_mut().zip(&wide[row * width..(row + 1) * width]) {
                *slot |= flag;
            }
        }
    });
    out
}

/// The offsets of the 8 neighbours with their weights.
fn neighbours() -> [(isize, isize, f32); 8] {
    let d = DIAGONAL_WEIGHT;
    [
        (-1, 0, 1.0),
        (1, 0, 1.0),
        (0, -1, 1.0),
        (0, 1, 1.0),
        (-1, -1, d),
        (1, -1, d),
        (-1, 1, d),
        (1, 1, d),
    ]
}

/// Fills the pixels flagged in `known == false` from the outside in (see the module docs).
fn fill(image: &mut ImageF32, known: &mut [bool]) {
    let (w, h) = (image.width as usize, image.height as usize);
    let mut pending: Vec<usize> = (0..known.len()).filter(|&i| !known[i]).collect();
    let around = neighbours();
    while !pending.is_empty() {
        let mut filled: Vec<(usize, [f32; 3])> = Vec::new();
        let mut waiting: Vec<usize> = Vec::new();
        for &index in &pending {
            let (x, y) = ((index % w) as isize, (index / w) as isize);
            let mut sum = [0.0f32; 3];
            let mut total = 0.0f32;
            for (dx, dy, weight) in around {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                    continue;
                }
                let at = ny as usize * w + nx as usize;
                if known[at] {
                    for (c, slot) in sum.iter_mut().enumerate() {
                        *slot += weight * image.data[at * 3 + c];
                    }
                    total += weight;
                }
            }
            if total > 0.0 {
                filled.push((index, sum.map(|s| s / total)));
            } else {
                waiting.push(index);
            }
        }
        if filled.is_empty() {
            // Nothing known is left to fill from (cannot happen with a known pixel somewhere).
            return;
        }
        // The pass reads only what was known before it, so the order within it does not matter.
        for (index, pixel) in filled {
            image.data[index * 3..index * 3 + 3].copy_from_slice(&pixel);
            known[index] = true;
        }
        pending = waiting;
    }
}

/// Removes the dust that the infrared plane `ir` (1 channel, the size of `image`) shows from the
/// 3-channel `image`, in place. Does nothing when `dust.enabled` is false. See the module docs.
pub fn remove_dust(
    image: &mut ImageF32,
    ir: &ImageF32,
    dust: &Dust,
) -> Result<DustReport, AppError> {
    require_rgb(image, "dust removal")?;
    if ir.channels != 1 || (ir.width, ir.height) != (image.width, image.height) {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The infrared plane does not fit the scan.",
        )
        .with_details(format!(
            "{}x{}x{} against {}x{}",
            ir.width, ir.height, ir.channels, image.width, image.height
        )));
    }
    let skipped = DustReport {
        masked: 0,
        applied: false,
    };
    if !dust.enabled {
        return Ok(skipped);
    }
    let clear = clear_level(&ir.data);
    if clear < MIN_CLEAR {
        return Ok(skipped);
    }
    let threshold = dust.threshold.clamp(0.0, 1.0);
    let cutoff = clear * (1.0 - threshold);
    let mask: Vec<bool> = ir
        .data
        .par_iter()
        .map(|&v| v.is_finite() && v < cutoff)
        .collect();
    let grown = dilate(&mask, image.width as usize, dust.grow_px as usize);
    let masked = grown.iter().filter(|&&m| m).count();
    if masked == 0 {
        return Ok(DustReport {
            masked,
            applied: true,
        });
    }
    if masked as f32 > MAX_MASKED_FRACTION * grown.len() as f32 {
        return Ok(DustReport {
            masked,
            applied: false,
        });
    }
    for v in &mut image.data {
        *v = if v.is_finite() {
            v.clamp(0.0, MAX_SAMPLE)
        } else {
            0.0
        };
    }
    let mut known: Vec<bool> = grown.iter().map(|&m| !m).collect();
    fill(image, &mut known);
    Ok(DustReport {
        masked,
        applied: true,
    })
}

#[cfg(test)]
mod tests {
    use gtp_core::testing::synthetic::{self, Params, Synthetic};

    use super::*;

    fn dust(threshold: f32, grow_px: u32) -> Dust {
        Dust {
            enabled: true,
            threshold,
            grow_px,
        }
    }

    fn scan(specks: u32) -> Synthetic {
        synthetic::negative(
            9,
            &Params {
                width: 160,
                height: 120,
                rebate: [10; 4],
                dust_specks: specks,
                with_ir: true,
                ..Params::default()
            },
        )
        .unwrap()
    }

    fn ir_of(s: &Synthetic) -> &ImageF32 {
        s.ir.as_ref().unwrap()
    }

    #[test]
    fn dusty_pixels_come_back_within_tolerance_of_the_clean_scan() {
        let dusty = scan(8);
        let clean = scan(0);
        let mut image = dusty.image.clone();
        let report = remove_dust(&mut image, ir_of(&dusty), &dust(0.5, 2)).unwrap();
        assert!(report.applied && report.masked > 0);
        let mut worst = 0.0f32;
        // Away from the edge of the picture: next to the rebate the fill has to mix in its colour.
        let (w, f) = (dusty.image.width as usize, dusty.truth.frame);
        let inside = |i: usize| {
            let (x, y) = ((i % w) as u32, (i / w) as u32);
            x >= f.x + 8 && x + 8 < f.x + f.w && y >= f.y + 8 && y + 8 < f.y + f.h
        };
        for (i, &flag) in dusty.truth.dust_mask.iter().enumerate() {
            if flag && inside(i) {
                for c in 0..3 {
                    let (got, want) = (image.data[i * 3 + c], clean.image.data[i * 3 + c]);
                    worst = worst.max((got - want).abs() / want.max(1e-3));
                }
            }
        }
        // The dust took 75 % of the light; the fill is flat across a spot, so a steep gradient shows.`n        assert!(worst < 0.25, "worst relative error {worst}");
        // Before, those pixels were a quarter of the clean value.
        let before = dusty
            .truth
            .dust_mask
            .iter()
            .position(|&f| f)
            .map(|i| dusty.image.data[i * 3] / clean.image.data[i * 3])
            .unwrap();
        assert!(before < 0.3);
    }

    #[test]
    fn pixels_outside_the_grown_mask_are_unchanged() {
        let dusty = scan(8);
        let mut image = dusty.image.clone();
        remove_dust(&mut image, ir_of(&dusty), &dust(0.5, 2)).unwrap();
        let (w, h) = (dusty.image.width as usize, dusty.image.height as usize);
        let flagged: Vec<bool> = dusty.truth.dust_mask.clone();
        let grown = dilate(&flagged, w, 2);
        assert_eq!(grown.len(), w * h);
        for (i, &near) in grown.iter().enumerate() {
            if !near {
                assert_eq!(
                    image.data[i * 3..i * 3 + 3],
                    dusty.image.data[i * 3..i * 3 + 3]
                );
            }
        }
    }

    #[test]
    fn a_higher_threshold_masks_fewer_pixels_and_more_grow_masks_more() {
        // A plane whose dust is of different depths.
        let mut data = vec![1.0f32; 64 * 64];
        for (i, depth) in [0.9f32, 0.6, 0.3, 0.1].into_iter().enumerate() {
            data[(10 + i * 12) * 64 + 20] = depth;
        }
        let ir = ImageF32::new(64, 64, 1, data).unwrap();
        let count = |threshold: f32, grow: u32| {
            let mut image = ImageF32::new(64, 64, 3, vec![0.5; 64 * 64 * 3]).unwrap();
            remove_dust(&mut image, &ir, &dust(threshold, grow))
                .unwrap()
                .masked
        };
        let by_threshold: Vec<usize> = [0.0, 0.2, 0.5, 0.8, 1.0].map(|t| count(t, 0)).to_vec();
        assert!(
            by_threshold.windows(2).all(|w| w[0] >= w[1]),
            "{by_threshold:?}"
        );
        assert!(by_threshold[0] > by_threshold[4]);
        let by_grow: Vec<usize> = (0..=4).map(|g| count(0.5, g)).collect();
        assert!(by_grow.windows(2).all(|w| w[0] < w[1]), "{by_grow:?}");
        assert_eq!(by_grow[0], 2);
        assert_eq!(by_grow[1], 2 * 9);
    }

    #[test]
    fn a_disabled_stage_and_a_clean_plane_change_nothing() {
        let dusty = scan(8);
        let mut image = dusty.image.clone();
        let off = Dust {
            enabled: false,
            ..dust(0.5, 2)
        };
        assert!(
            !remove_dust(&mut image, ir_of(&dusty), &off)
                .unwrap()
                .applied
        );
        assert_eq!(image, dusty.image);
        let clean = scan(0);
        let mut image = clean.image.clone();
        let report = remove_dust(&mut image, ir_of(&clean), &dust(0.5, 2)).unwrap();
        assert_eq!(report.masked, 0);
        assert_eq!(image, clean.image);
    }

    #[test]
    fn a_plane_that_is_mostly_dark_is_ignored() {
        let dusty = scan(0);
        let half = ImageF32::new(
            160,
            120,
            1,
            (0..160 * 120)
                .map(|i| if i % 160 < 60 { 0.05 } else { 1.0 })
                .collect(),
        )
        .unwrap();
        let mut image = dusty.image.clone();
        let report = remove_dust(&mut image, &half, &dust(0.5, 2)).unwrap();
        assert!(!report.applied);
        assert_eq!(image, dusty.image);
        let black = ImageF32::new(160, 120, 1, vec![0.0; 160 * 120]).unwrap();
        assert!(
            !remove_dust(&mut image, &black, &dust(0.5, 2))
                .unwrap()
                .applied
        );
        assert_eq!(image, dusty.image);
    }

    #[test]
    fn a_spot_at_the_corner_and_a_large_spot_are_filled() {
        let mut data = vec![1.0f32; 40 * 40];
        for y in 0..3 {
            for x in 0..3 {
                data[y * 40 + x] = 0.0;
            }
        }
        for y in 15..25 {
            for x in 15..25 {
                data[y * 40 + x] = 0.0;
            }
        }
        let ir = ImageF32::new(40, 40, 1, data).unwrap();
        let mut image = ImageF32::new(40, 40, 3, vec![0.4; 40 * 40 * 3]).unwrap();
        for y in 15..25 {
            for x in 15..25 {
                image.data[(y * 40 + x) * 3..(y * 40 + x) * 3 + 3].fill(0.0);
            }
        }
        image.data[..3].fill(0.0);
        remove_dust(&mut image, &ir, &dust(0.5, 1)).unwrap();
        assert!(image.data.iter().all(|&v| (v - 0.4).abs() < 1e-5));
    }

    #[test]
    fn a_plane_of_the_wrong_shape_is_refused() {
        let mut image = ImageF32::new(8, 8, 3, vec![0.5; 192]).unwrap();
        let small = ImageF32::new(4, 8, 1, vec![1.0; 32]).unwrap();
        let err = remove_dust(&mut image, &small, &dust(0.5, 2)).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let colour = ImageF32::new(8, 8, 3, vec![1.0; 192]).unwrap();
        assert!(remove_dust(&mut image, &colour, &dust(0.5, 2)).is_err());
    }

    /// 21 megapixels (5616 x 3744) with 2,000 specks of radius 3 and the default settings; run with
    /// `cargo test -p gtp-edit --release -- --ignored --nocapture time_a_21_megapixel` to see the
    /// time.
    #[test]
    #[ignore = "timing run, not a check"]
    fn time_a_21_megapixel_dust_removal() {
        let (w, h) = (5616usize, 3744usize);
        let mut plane = vec![1.0f32; w * h];
        let mut state = 12_345u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        for _ in 0..2000 {
            let (cx, cy) = (next() as usize % (w - 8) + 4, next() as usize % (h - 8) + 4);
            for y in cy - 3..=cy + 3 {
                for x in cx - 3..=cx + 3 {
                    plane[y * w + x] = 0.1;
                }
            }
        }
        let ir = ImageF32::new(w as u32, h as u32, 1, plane).unwrap();
        let mut image = ImageF32::new(
            w as u32,
            h as u32,
            3,
            (0..w * h * 3).map(|i| (i % 1000) as f32 / 1000.0).collect(),
        )
        .unwrap();
        let start = std::time::Instant::now();
        let report = remove_dust(&mut image, &ir, &dust(0.5, 2)).unwrap();
        eprintln!("21 MP dust removal, {report:?}: {:?}", start.elapsed());
    }
}
