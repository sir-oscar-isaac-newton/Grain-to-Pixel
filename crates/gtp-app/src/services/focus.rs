//! The focus check and the film flatness map (H-19): the sharpness of a region of a linear
//! plane, a 3 x 3 map of it over the frame, the pattern the map shows (flat, bowed, tilted), the
//! comparison with a saved flat reference, and the 1:1 crop as a JPEG for the screen.
//!
//! Sharpness is the variance of a 3 x 3 Laplacian divided by the square of the region's mean, so a
//! brighter or darker region (an uneven light, a dense part of the film) does not look sharper or
//! softer. It still depends on the content: the map is fair only on an evenly textured frame.

use gtp_core::recipe::OutputSpace;
use gtp_core::{AppError, ErrorCode, ImageF32};
use gtp_io::encode_jpeg_bytes;
use serde::{Deserialize, Serialize};

/// The size of the crop shown 1:1, in pixels (smaller when the plane is smaller).
pub const CROP_SIDE_PX: usize = 512;
/// The JPEG quality of the crop.
const CROP_QUALITY: u8 = 90;
/// The crop is normalised by this percentile of its values before it is drawn.
const CROP_PERCENTILE: f32 = 0.995;
/// A region whose mean is below this (linear, 0 to 1) has no usable sharpness and counts as 0.
pub const MIN_MEAN: f64 = 1e-4;

/// Flat: every region is at least this much of the best one.
pub const FLAT_MIN: f32 = 0.85;
/// Bowed: the centre is the best or within this of it.
pub const BOWED_CENTRE_TOLERANCE: f32 = 0.05;
/// Bowed: the outer regions are lower than the centre by this much on average.
pub const BOWED_EDGE_DROP: f32 = 0.15;
/// Bowed: the outer regions differ from each other by at most this much (max minus min).
pub const BOWED_OUTER_SPREAD: f32 = 0.2;
/// Tilted: one side (or corner) is lower than the opposite one by at least this much.
pub const TILT_MIN_DIFFERENCE: f32 = 0.2;
/// Tilted: a corner is reported instead of a side only when its difference from the opposite
/// corner is this much larger than the best side's (a clear gradient across a column also makes
/// its corners differ, and that is a tilt of the side).
pub const TILT_CORNER_MARGIN: f32 = 0.2;

/// A 3 x 3 grid of values, row-major (rows from the top, columns from the left).
pub type Map = [[f32; 3]; 3];

/// Which side or corner of the frame is softer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum TiltSide {
    /// The left edge.
    Left,
    /// The right edge.
    Right,
    /// The top edge.
    Top,
    /// The bottom edge.
    Bottom,
    /// The top left corner.
    TopLeft,
    /// The top right corner.
    TopRight,
    /// The bottom left corner.
    BottomLeft,
    /// The bottom right corner.
    BottomRight,
}

/// What the map of sharpness shows about how the film lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FocusPattern {
    /// Even sharpness over the frame.
    Flat,
    /// The middle is sharp and the edges are evenly softer.
    Bowed,
    /// One side or corner is softer than the opposite one.
    Tilted {
        /// The softer side or corner.
        side: TiltSide,
    },
    /// None of the above.
    Unclear,
}

/// The sharpness of the part of `plane` (one channel, `width` values a row) from `x0, y0` up to
/// but not including `x1, y1`: the variance of the 4-neighbour Laplacian of the pixels that have
/// all their neighbours inside the part, divided by the square of their mean. 0 for a part
/// smaller than 3 x 3 pixels or with a mean near zero.
pub fn region_sharpness(
    plane: &[f32],
    width: usize,
    (x0, y0, x1, y1): (usize, usize, usize, usize),
) -> f32 {
    if x1 < x0 + 3 || y1 < y0 + 3 {
        return 0.0;
    }
    let (mut sum_value, mut sum_lap, mut sum_lap_sq, mut count) = (0.0f64, 0.0f64, 0.0f64, 0u64);
    for y in y0 + 1..y1 - 1 {
        let above = &plane[(y - 1) * width + x0..(y - 1) * width + x1];
        let row = &plane[y * width + x0..y * width + x1];
        let below = &plane[(y + 1) * width + x0..(y + 1) * width + x1];
        for x in 1..row.len() - 1 {
            let lap = above[x] + below[x] + row[x - 1] + row[x + 1] - 4.0 * row[x];
            let lap = f64::from(lap);
            sum_value += f64::from(row[x]);
            sum_lap += lap;
            sum_lap_sq += lap * lap;
            count += 1;
        }
    }
    let n = count as f64;
    let mean = sum_value / n;
    if mean.is_nan() || mean < MIN_MEAN {
        return 0.0;
    }
    let variance = (sum_lap_sq / n - (sum_lap / n).powi(2)).max(0.0);
    (variance / (mean * mean)) as f32
}

/// The sharpness of each of the 3 x 3 equal regions of `plane`, as measured (not relative).
pub fn sharpness_grid(plane: &ImageF32) -> Map {
    let (width, height) = (plane.width as usize, plane.height as usize);
    let edge = |n: usize, i: usize| n * i / 3;
    let mut grid = [[0.0; 3]; 3];
    for (row, cells) in grid.iter_mut().enumerate() {
        for (col, cell) in cells.iter_mut().enumerate() {
            let bounds = (
                edge(width, col),
                edge(height, row),
                edge(width, col + 1),
                edge(height, row + 1),
            );
            *cell = region_sharpness(&plane.data, width, bounds);
        }
    }
    grid
}

/// `map` divided by its best region, so the best is 1. A map with no positive value stays as it is.
pub fn relative_to_best(map: &Map) -> Map {
    let best = map.iter().flatten().copied().fold(0.0f32, f32::max);
    if best <= 0.0 {
        return *map;
    }
    map.map(|row| row.map(|v| v / best))
}

/// `map` divided region by region by `reference` (the lens's own field curvature, from a flat
/// target), then relative to the best again.
pub fn against_reference(map: &Map, reference: &Map) -> Map {
    let mut divided = *map;
    for (row, ref_row) in divided.iter_mut().zip(reference) {
        for (value, r) in row.iter_mut().zip(ref_row) {
            *value /= r;
        }
    }
    relative_to_best(&divided)
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len() as f32
}

/// The pattern of a map that is relative to its best region (best = 1).
pub fn classify(map: &Map) -> FocusPattern {
    let all = map.iter().flatten().copied().collect::<Vec<_>>();
    if all.iter().all(|v| *v >= FLAT_MIN) {
        return FocusPattern::Flat;
    }
    let best = all.iter().copied().fold(0.0f32, f32::max);
    let centre = map[1][1];
    let outer: Vec<f32> = [
        map[0][0], map[0][1], map[0][2], map[1][0], map[1][2], map[2][0], map[2][1], map[2][2],
    ]
    .to_vec();
    let outer_min = outer.iter().copied().fold(f32::MAX, f32::min);
    let outer_max = outer.iter().copied().fold(f32::MIN, f32::max);
    if centre >= best - BOWED_CENTRE_TOLERANCE
        && mean(&outer) <= centre - BOWED_EDGE_DROP
        && outer_max - outer_min <= BOWED_OUTER_SPREAD
    {
        return FocusPattern::Bowed;
    }
    let column = |c: usize| mean(&[map[0][c], map[1][c], map[2][c]]);
    let row = |r: usize| mean(&map[r]);
    let sides = [
        (TiltSide::Left, column(2) - column(0)),
        (TiltSide::Right, column(0) - column(2)),
        (TiltSide::Top, row(2) - row(0)),
        (TiltSide::Bottom, row(0) - row(2)),
    ];
    let corners = [
        (TiltSide::TopLeft, map[2][2] - map[0][0]),
        (TiltSide::BottomRight, map[0][0] - map[2][2]),
        (TiltSide::TopRight, map[2][0] - map[0][2]),
        (TiltSide::BottomLeft, map[0][2] - map[2][0]),
    ];
    let strongest = |candidates: [(TiltSide, f32); 4]| {
        candidates
            .into_iter()
            .fold(None::<(TiltSide, f32)>, |best, c| match best {
                Some((_, d)) if d >= c.1 => best,
                _ => Some(c),
            })
    };
    let side = strongest(sides).filter(|(_, d)| *d >= TILT_MIN_DIFFERENCE);
    let corner = strongest(corners).filter(|(_, d)| *d >= TILT_MIN_DIFFERENCE);
    match (side, corner) {
        (Some((_, side_d)), Some((corner, corner_d)))
            if corner_d >= side_d + TILT_CORNER_MARGIN =>
        {
            FocusPattern::Tilted { side: corner }
        }
        (Some((side, _)), _) => FocusPattern::Tilted { side },
        (None, Some((corner, _))) => FocusPattern::Tilted { side: corner },
        (None, None) => FocusPattern::Unclear,
    }
}

/// A saved reference must have only finite values above zero.
pub fn validate_reference(reference: &Map) -> Result<(), AppError> {
    if reference
        .iter()
        .flatten()
        .all(|v| v.is_finite() && *v > 0.0)
    {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::InvalidInput,
            "A flat reference needs all nine regions to have a sharpness above zero. Take it again on an evenly textured flat target.",
        ))
    }
}

/// The bounds of the crop of a `width` x `height` plane: its middle, [`CROP_SIDE_PX`] square at most.
pub fn crop_bounds(width: usize, height: usize) -> (usize, usize, usize, usize) {
    let (w, h) = (width.min(CROP_SIDE_PX), height.min(CROP_SIDE_PX));
    let (x0, y0) = ((width - w) / 2, (height - h) / 2);
    (x0, y0, x0 + w, y0 + h)
}

/// The crop of `plane` inside `bounds` as a grey JPEG: its values are divided by their 99.5th
/// percentile (so the picture fills the range whatever the exposure) and drawn through the sRGB
/// curve, 1:1.
pub fn crop_jpeg(
    plane: &ImageF32,
    (x0, y0, x1, y1): (usize, usize, usize, usize),
) -> Result<Vec<u8>, AppError> {
    let width = plane.width as usize;
    let mut values = Vec::with_capacity((x1 - x0) * (y1 - y0));
    for y in y0..y1 {
        values.extend_from_slice(&plane.data[y * width + x0..y * width + x1]);
    }
    let mut sorted: Vec<f32> = values
        .iter()
        .map(|v| if v.is_finite() { *v } else { 0.0 })
        .collect();
    let rank =
        ((sorted.len() as f32 * CROP_PERCENTILE) as usize).min(sorted.len().saturating_sub(1));
    let top = if sorted.is_empty() {
        0.0
    } else {
        *sorted.select_nth_unstable_by(rank, f32::total_cmp).1
    };
    let scale = if top > 0.0 { 1.0 / top } else { 0.0 };
    let mut data = Vec::with_capacity(values.len() * 3);
    for v in &values {
        let grey = if v.is_finite() {
            (v * scale).clamp(0.0, 1.0)
        } else {
            0.0
        };
        data.extend_from_slice(&[grey, grey, grey]);
    }
    let image = ImageF32::new((x1 - x0) as u32, (y1 - y0) as u32, 3, data)?;
    encode_jpeg_bytes(&image, OutputSpace::Srgb, CROP_QUALITY)
}

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// What one focus check measured on a plane.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// The crop as JPEG bytes.
    pub crop: Vec<u8>,
    /// The crop's size in pixels.
    pub crop_size: (u32, u32),
    /// The sharpness of the crop.
    pub sharpness: f32,
    /// The map relative to its best region, without any reference.
    pub raw_map: Map,
    /// The map shown: against the reference when there is one.
    pub map: Map,
    /// The pattern of `map`.
    pub pattern: FocusPattern,
}

/// Measures `plane`: the crop of its middle, its sharpness, the map and its pattern, the map
/// against `reference` when one is given.
pub fn analyse(plane: &ImageF32, reference: Option<&Map>) -> Result<Analysis, AppError> {
    let (width, height) = (plane.width as usize, plane.height as usize);
    let bounds = crop_bounds(width, height);
    let crop = crop_jpeg(plane, bounds)?;
    let sharpness = region_sharpness(&plane.data, width, bounds);
    let grid = sharpness_grid(plane);
    let raw_map = relative_to_best(&grid);
    let map = match reference {
        Some(reference) => against_reference(&grid, reference),
        None => raw_map,
    };
    Ok(Analysis {
        crop,
        crop_size: ((bounds.2 - bounds.0) as u32, (bounds.3 - bounds.1) as u32),
        sharpness,
        raw_map,
        map,
        pattern: classify(&map),
    })
}

#[cfg(test)]
#[path = "focus_tests.rs"]
mod tests;
