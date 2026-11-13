//! Noise reduction stage (docs/specification.md section 4.2, after step 4, the inversion): a guided
//! filter (He, Sun and Tang) in Oklab, with no learned model (ADR 0013).
//!
//! The positive is converted to the Oklab planes `L`, `a` and `b` (see [`crate::oklab`]).
//!
//! **Guided filter.** For a plane `p`, a guide `I`, a radius `r` and a regulariser `eps`, with
//! `mean` the average over the `(2r+1)^2` window clipped at the image border (divided by the real
//! number of pixels in the window): `a_k = (mean(I p) - mean(I) mean(p)) / (var(I) + eps)`,
//! `b_k = mean(p) - a_k mean(I)` and `q = mean(a_k) I + mean(b_k)`. Where the guide has a variance
//! well above `eps` (an edge) `q` follows `p`; where it is flat, `q` tends to a local mean.
//!
//! **Luminance.** `L' = guided(p = L, I = L, r_l, e_l)`.
//!
//! **Colour.** `a' = guided(p = a, I = L', r_c, e_c)` and likewise `b'`, so the colour is only
//! smoothed over areas where the (cleaned) lightness is flat, and a colour edge that comes with a
//! lightness edge is kept. The guide is the unfiltered `L` when the luminance amount is 0.
//!
//! **Scale.** `scale` is the size of the image being processed relative to the full scan (1 at
//! export, below 1 for a preview made from a smaller proxy). Radii and noise levels are given for
//! the full scan and scale with the image: `r = round(R * scale)`, at least 1 pixel, and
//! `e = (amount / 100 * MAX_SIGMA * scale)^2`. A preview can only approximate the full-size
//! result: fine grain is averaged away by the downscaling, and the smaller radii are whole pixels.
//!
//! Box averages cost a constant per pixel whatever the radius: a sliding window sum along the rows,
//! then along the columns, per band of rows (bands in parallel, with a scratch of their own, so no
//! full-size buffer holds the row sums). Each output sample depends only on its own band, summed in
//! a fixed order in `f64`, so the result is the same on any number of threads. The shifted planes,
//! products, slope and offset of the guided filter are computed row by row inside the box averages,
//! and the full-size planes are reused between the steps of a reduction (`Workspace`).
//!
//! The result is converted back to linear Rec.2020 and not clamped. Both amounts at 0 return the
//! image untouched, bit for bit. A pixel with a non-finite value is left as it is and enters the
//! averages as 0, so it can not spread.

use gtp_core::recipe::{Noise, NOISE_AMOUNT_MAX};
use gtp_core::{AppError, ErrorCode, ImageF32};
use rayon::prelude::*;

use crate::oklab::{oklab_to_rgb, rgb_to_oklab};
use crate::require_rgb;

/// Radius, in pixels of the full scan, of the luminance filter. Like every edit constant it is to
/// be tuned on real scans (issue Q-03).
pub const NR_LUMA_RADIUS_PX: f32 = 3.0;
/// Radius, in pixels of the full scan, of the colour filter (to be tuned on real scans).
pub const NR_CHROMA_RADIUS_PX: f32 = 8.0;
/// Standard deviation, in Oklab lightness, that the luminance filter treats as noise at an amount
/// of 100 (to be tuned on real scans).
pub const NR_LUMA_MAX_SIGMA: f32 = 0.04;
/// The same for the colour filter, in Oklab `a` and `b` units (to be tuned on real scans).
pub const NR_CHROMA_MAX_SIGMA: f32 = 0.06;

/// Samples per task when a reduction over a plane is split, a fixed size so the order of the
/// partial sums never depends on the number of threads.
const SUM_CHUNK: usize = 4096;
/// Rows per parallel task of the vertical pass of the box average.
const COLUMN_BAND: usize = 64;

fn limited(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, NOISE_AMOUNT_MAX)
    } else {
        0.0
    }
}

/// A radius in whole pixels for the given scale, at least 1.
fn scaled_radius(radius_px: f32, scale: f32) -> usize {
    (radius_px * scale).round().max(1.0) as usize
}

/// The regulariser of an amount (0 to 100) for the given scale.
fn regulariser(amount: f32, max_sigma: f32, scale: f32) -> f32 {
    let sigma = amount / NOISE_AMOUNT_MAX * max_sigma * scale;
    sigma * sigma
}

/// One row of window sums along a row: for each sample, the sum of the samples within `r` of it,
/// clipped at the row ends. Sliding sum in `f64`; each sum is rounded to `f32` when stored.
fn sum_row(row: &[f32], dst: &mut [f32], r: usize) {
    let width = row.len();
    let mut acc: f64 = row[..=r.min(width - 1)].iter().map(|v| *v as f64).sum();
    for x in 0..width {
        dst[x] = acc as f32;
        if x + 1 + r < width {
            acc += row[x + 1 + r] as f64;
        }
        if x >= r {
            acc -= row[x - r] as f64;
        }
    }
}

/// Number of samples of a window of radius `r` around each index of a line of `len` samples.
fn window_counts(len: usize, r: usize) -> Vec<f32> {
    (0..len)
        .map(|i| (i.saturating_add(r).min(len - 1) - i.saturating_sub(r) + 1) as f32)
        .collect()
}

/// Scratch of one band task of [`box_mean_into`]: one source row, the row sums of the rows the
/// band needs and the sliding column sums.
#[derive(Default)]
struct BandScratch {
    line: Vec<f32>,
    sums: Vec<f32>,
    acc: Vec<f64>,
}

/// Full-size `f32` planes kept between the box averages and the passes of one noise reduction, so
/// none is allocated and freed again for each step. A buffer comes back with arbitrary contents;
/// every user overwrites all of it.
#[derive(Default)]
struct Workspace {
    free: Vec<Vec<f32>>,
}

impl Workspace {
    fn take(&mut self, len: usize) -> Vec<f32> {
        let mut plane = self.free.pop().unwrap_or_default();
        plane.resize(len, 0.0);
        plane
    }

    fn give(&mut self, plane: Vec<f32>) {
        self.free.push(plane);
    }
}

/// The average over the `(2r+1)^2` window around each sample, clipped at the border and divided by
/// the real number of samples in the window, of the plane whose row `y` `fill(y, line)` writes into
/// `line` (`width` samples). `out` has `width * height` samples and is overwritten.
///
/// The rows are cut into bands of [`COLUMN_BAND`] that run in parallel. A band slides a window sum
/// along each row it needs (rows `first - r` to `last + 1 + r`, clipped), stored as `f32`, then
/// slides sums in `f64` down the columns, started from the rows of its first window, so the result
/// does not depend on the number of threads. The row sums of the rows two bands share are
/// computed in both; they come out the same.
fn box_mean_into(
    out: &mut [f32],
    width: usize,
    height: usize,
    r: usize,
    fill: impl Fn(usize, &mut [f32]) + Sync,
) {
    if width == 0 || height == 0 {
        return;
    }
    let across = window_counts(width, r);
    let down = window_counts(height, r);
    out.par_chunks_mut(COLUMN_BAND * width)
        .enumerate()
        .for_each_init(BandScratch::default, |scratch, (band, dst)| {
            let BandScratch { line, sums, acc } = scratch;
            let first = band * COLUMN_BAND;
            let last = first + dst.len() / width - 1;
            let low = first.saturating_sub(r);
            let high = (last + 1 + r).min(height - 1);
            line.resize(width, 0.0);
            sums.resize((high - low + 1) * width, 0.0);
            for y in low..=high {
                fill(y, line);
                sum_row(line, &mut sums[(y - low) * width..(y - low + 1) * width], r);
            }
            let sums_of = |row: usize| &sums[(row - low) * width..(row - low + 1) * width];
            acc.clear();
            acc.resize(width, 0.0);
            for row in first.saturating_sub(r)..=(first + r).min(height - 1) {
                for (a, v) in acc.iter_mut().zip(sums_of(row)) {
                    *a += *v as f64;
                }
            }
            for (i, out_line) in dst.chunks_mut(width).enumerate() {
                let y = first + i;
                for ((o, a), n) in out_line.iter_mut().zip(acc.iter()).zip(&across) {
                    *o = (*a as f32) / (n * down[y]);
                }
                if y + 1 + r < height {
                    for (a, v) in acc.iter_mut().zip(sums_of(y + 1 + r)) {
                        *a += *v as f64;
                    }
                }
                if y >= r {
                    for (a, v) in acc.iter_mut().zip(sums_of(y - r)) {
                        *a -= *v as f64;
                    }
                }
            }
        });
}

/// The average over the `(2r+1)^2` window around each sample of `src`, clipped at the border and
/// divided by the real number of samples in the window.
#[cfg(test)]
pub(crate) fn box_mean(src: &[f32], width: usize, height: usize, r: usize) -> Vec<f32> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let mut out = vec![0.0_f32; width * height];
    box_mean_into(&mut out, width, height, r, |y, line| {
        line.copy_from_slice(&src[y * width..(y + 1) * width]);
    });
    out
}

/// The mean of a plane, summed in `f64` over fixed chunks in a fixed order.
fn plane_mean(plane: &[f32]) -> f32 {
    if plane.is_empty() {
        return 0.0;
    }
    let partial: Vec<f64> = plane
        .par_chunks(SUM_CHUNK)
        .map(|chunk| chunk.iter().map(|v| *v as f64).sum())
        .collect();
    (partial.iter().sum::<f64>() / plane.len() as f64) as f32
}

/// Row `y` of a plane `width` samples wide.
fn row_of(plane: &[f32], width: usize, y: usize) -> &[f32] {
    &plane[y * width..(y + 1) * width]
}

/// The slope `a_k` of one row from the row's box means: `products` of guide and plane, `guide`,
/// `plane` and `squares` of the guide (the variance is `squares - guide^2`, not below 0).
fn slope_row(
    out: &mut [f32],
    products: &[f32],
    guide: &[f32],
    plane: &[f32],
    squares: &[f32],
    eps: f32,
) {
    for i in 0..out.len() {
        let variance = (squares[i] - guide[i] * guide[i]).max(0.0);
        let covariance = products[i] - guide[i] * plane[i];
        let denominator = variance + eps;
        out[i] = if denominator > 0.0 {
            covariance / denominator
        } else {
            0.0
        };
    }
}

/// The guided filter of each of `planes` with the same `guide`, radius and regulariser. A plane
/// that is the guide itself (the same slice) is filtered with the cheaper self-guided form.
///
/// Guide and planes are shifted by their means before the sums, which the filter does not mind
/// (it is shift invariant) and which keeps the variance free of rounding. The shifted planes, the
/// products, the slope and the offset are never stored: each box average computes the rows it
/// needs from the inputs, with the same expression, so the values are the same as if stored.
fn guided_filter_in(
    ws: &mut Workspace,
    guide: &[f32],
    planes: &[&[f32]],
    width: usize,
    height: usize,
    r: usize,
    eps: f32,
) -> Vec<Vec<f32>> {
    let count = width * height;
    let shift_guide = plane_mean(guide);
    let mut mean_guide = ws.take(count);
    box_mean_into(&mut mean_guide, width, height, r, |y, line| {
        for (o, g) in line.iter_mut().zip(row_of(guide, width, y)) {
            *o = *g - shift_guide;
        }
    });
    let mut mean_squares = ws.take(count);
    box_mean_into(&mut mean_squares, width, height, r, |y, line| {
        for (o, g) in line.iter_mut().zip(row_of(guide, width, y)) {
            let centred = *g - shift_guide;
            *o = centred * centred;
        }
    });

    let result = planes
        .iter()
        .map(|plane| {
            let own = std::ptr::eq(plane.as_ptr(), guide.as_ptr());
            let shift = if own { shift_guide } else { plane_mean(plane) };
            let mut computed_p = None;
            let mut computed_products = None;
            if !own {
                let mut mean_p = ws.take(count);
                box_mean_into(&mut mean_p, width, height, r, |y, line| {
                    for (o, v) in line.iter_mut().zip(row_of(plane, width, y)) {
                        *o = *v - shift;
                    }
                });
                let mut products = ws.take(count);
                box_mean_into(&mut products, width, height, r, |y, line| {
                    let pairs = row_of(guide, width, y).iter().zip(row_of(plane, width, y));
                    for (o, (g, v)) in line.iter_mut().zip(pairs) {
                        *o = (*g - shift_guide) * (*v - shift);
                    }
                });
                computed_p = Some(mean_p);
                computed_products = Some(products);
            }
            let mean_p: &[f32] = computed_p.as_deref().unwrap_or(&mean_guide);
            let mean_products: &[f32] = computed_products.as_deref().unwrap_or(&mean_squares);
            let slope_of = |y: usize, line: &mut [f32]| {
                slope_row(
                    line,
                    row_of(mean_products, width, y),
                    row_of(&mean_guide, width, y),
                    row_of(mean_p, width, y),
                    row_of(&mean_squares, width, y),
                    eps,
                );
            };

            let mut mean_slope = ws.take(count);
            box_mean_into(&mut mean_slope, width, height, r, slope_of);
            let mut mean_offset = ws.take(count);
            box_mean_into(&mut mean_offset, width, height, r, |y, line| {
                slope_of(y, line);
                let guide_row = row_of(&mean_guide, width, y);
                for ((o, p), g) in line.iter_mut().zip(row_of(mean_p, width, y)).zip(guide_row) {
                    *o = *p - *o * *g;
                }
            });
            mean_slope
                .par_iter_mut()
                .zip(mean_offset.par_iter())
                .zip(guide.par_iter())
                .for_each(|((q, offset), g)| {
                    let centred = *g - shift_guide;
                    *q = *q * centred + *offset + shift;
                });
            ws.give(mean_offset);
            if let Some(buffer) = computed_p {
                ws.give(buffer);
            }
            if let Some(buffer) = computed_products {
                ws.give(buffer);
            }
            mean_slope
        })
        .collect();
    ws.give(mean_guide);
    ws.give(mean_squares);
    result
}

/// [`guided_filter_in`] with a workspace of its own.
#[cfg(test)]
pub(crate) fn guided_filter(
    guide: &[f32],
    planes: &[&[f32]],
    width: usize,
    height: usize,
    r: usize,
    eps: f32,
) -> Vec<Vec<f32>> {
    guided_filter_in(
        &mut Workspace::default(),
        guide,
        planes,
        width,
        height,
        r,
        eps,
    )
}

/// Reduces the noise of `image` (linear Rec.2020, 3 channels) in place; see the module docs.
/// `scale` is the size of `image` relative to the full scan: 1 at export, below 1 for a preview.
/// The amounts are limited to 0 to 100 here as well, so no input can produce NaN.
pub fn apply_noise_reduction(
    image: &mut ImageF32,
    noise: &Noise,
    scale: f32,
) -> Result<(), AppError> {
    require_rgb(image, "noise reduction")?;
    if !scale.is_finite() || scale <= 0.0 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The noise reduction stage needs a positive scale.",
        )
        .with_details(scale.to_string()));
    }
    let (luminance, color) = (limited(noise.luminance), limited(noise.color));
    if (luminance == 0.0 && color == 0.0) || image.data.is_empty() {
        return Ok(());
    }
    let (width, height) = (image.width as usize, image.height as usize);
    let count = width * height;

    // The Oklab planes; a pixel that is not finite enters as 0 and is restored at the end.
    let (mut l, mut a, mut b) = (vec![0.0; count], vec![0.0; count], vec![0.0; count]);
    l.par_chunks_mut(width)
        .zip(a.par_chunks_mut(width))
        .zip(b.par_chunks_mut(width))
        .zip(image.data.par_chunks(width * 3))
        .for_each(|(((l, a), b), row)| {
            for (i, pixel) in row.as_chunks::<3>().0.iter().enumerate() {
                if pixel.iter().all(|c| c.is_finite()) {
                    [l[i], a[i], b[i]] = rgb_to_oklab(*pixel);
                }
            }
        });

    let mut workspace = Workspace::default();
    if luminance > 0.0 {
        let r = scaled_radius(NR_LUMA_RADIUS_PX, scale);
        let eps = regulariser(luminance, NR_LUMA_MAX_SIGMA, scale);
        let mut out = guided_filter_in(&mut workspace, &l, &[&l], width, height, r, eps);
        workspace.give(std::mem::replace(&mut l, out.swap_remove(0)));
    }
    if color > 0.0 {
        let r = scaled_radius(NR_CHROMA_RADIUS_PX, scale);
        let eps = regulariser(color, NR_CHROMA_MAX_SIGMA, scale);
        let mut out = guided_filter_in(&mut workspace, &l, &[&a, &b], width, height, r, eps);
        workspace.give(std::mem::replace(&mut b, out.swap_remove(1)));
        workspace.give(std::mem::replace(&mut a, out.swap_remove(0)));
    }

    image
        .data
        .par_chunks_mut(width * 3)
        .zip(l.par_chunks(width))
        .zip(a.par_chunks(width))
        .zip(b.par_chunks(width))
        .for_each(|(((row, l), a), b)| {
            for (i, pixel) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                if !pixel.iter().all(|c| c.is_finite()) {
                    continue;
                }
                let out = oklab_to_rgb([l[i], a[i], b[i]]);
                if out.iter().all(|c| c.is_finite()) {
                    *pixel = out;
                }
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(seed: u32) -> impl FnMut() -> f32 {
        let mut state = seed;
        move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32
        }
    }

    /// Noise with a standard deviation of `sigma`: the sum of four uniform values.
    fn gauss(next: &mut impl FnMut() -> f32, sigma: f32) -> f32 {
        let sum: f32 = (0..4).map(|_| next()).sum();
        (sum - 2.0) * sigma * 3.0_f32.sqrt()
    }

    fn random_plane(width: usize, height: usize, seed: u32) -> Vec<f32> {
        let mut next = stream(seed);
        (0..width * height).map(|_| next()).collect()
    }

    fn on_one_thread<T: Send>(f: impl FnOnce() -> T + Send) -> T {
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(f)
    }

    /// The box average straight from its definition, in `f64`.
    fn direct_box_mean(src: &[f32], width: usize, height: usize, r: usize) -> Vec<f64> {
        let mut out = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let (x0, x1) = (x.saturating_sub(r), (x + r).min(width - 1));
                let (y0, y1) = (y.saturating_sub(r), (y + r).min(height - 1));
                let mut sum = 0.0_f64;
                for yy in y0..=y1 {
                    for xx in x0..=x1 {
                        sum += src[yy * width + xx] as f64;
                    }
                }
                out.push(sum / ((x1 - x0 + 1) * (y1 - y0 + 1)) as f64);
            }
        }
        out
    }

    fn max_difference(a: &[f32], b: &[f64]) -> f64 {
        a.iter()
            .zip(b)
            .map(|(x, y)| (*x as f64 - y).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn the_box_average_matches_the_direct_computation_including_the_borders() {
        let (width, height) = (41, 29);
        let plane = random_plane(width, height, 7);
        // Radii 1, 3 and 8, and one larger than the image (the window is clipped everywhere).
        for r in [1, 3, 8, 60] {
            let fast = box_mean(&plane, width, height, r);
            let direct = direct_box_mean(&plane, width, height, r);
            let error = max_difference(&fast, &direct);
            assert!(error < 1e-6, "radius {r}: error {error}");
        }
    }

    #[test]
    fn the_box_average_of_a_long_row_of_values_near_one_stays_accurate() {
        // A 6000-wide row of values near 1: the sliding sums (f64) stay close to the direct ones;
        // the only error left is the rounding of the f32 result.
        let (width, height) = (6000, 3);
        let mut next = stream(3);
        let plane: Vec<f32> = (0..width * height).map(|_| 0.9 + 0.2 * next()).collect();
        for r in [3, 8] {
            let fast = box_mean(&plane, width, height, r);
            let direct = direct_box_mean(&plane, width, height, r);
            let error = max_difference(&fast, &direct);
            eprintln!("6000 wide, radius {r}: largest error {error:e}");
            assert!(error < 2e-7, "radius {r}: error {error}");
        }
    }

    #[test]
    fn the_box_average_is_the_same_on_one_thread_as_on_many() {
        let (width, height) = (301, 157);
        let plane = random_plane(width, height, 11);
        let many = box_mean(&plane, width, height, 8);
        let one = on_one_thread(|| box_mean(&plane, width, height, 8));
        assert_eq!(one, many);
    }

    #[test]
    fn the_box_average_of_an_empty_plane_is_empty() {
        assert!(box_mean(&[], 0, 0, 3).is_empty());
    }

    fn filtered(plane: &[f32], width: usize, height: usize, r: usize, eps: f32) -> Vec<f32> {
        guided_filter(plane, &[plane], width, height, r, eps).remove(0)
    }

    #[test]
    fn the_guided_filter_leaves_a_constant_plane_unchanged() {
        let plane = vec![0.37_f32; 40 * 30];
        for eps in [0.0, 1e-6, 1e-2] {
            let out = filtered(&plane, 40, 30, 3, eps);
            let error = out.iter().map(|v| (v - 0.37).abs()).fold(0.0, f32::max);
            assert!(error < 1e-6, "eps {eps}: error {error}");
        }
    }

    #[test]
    fn a_large_regulariser_turns_a_flat_noisy_plane_into_its_box_mean() {
        let (width, height) = (64, 48);
        let mut next = stream(5);
        let plane: Vec<f32> = (0..width * height)
            .map(|_| 0.5 + gauss(&mut next, 0.02))
            .collect();
        let out = filtered(&plane, width, height, 3, 1e3);
        // With the slope at 0 the result is the box average of the box average.
        let twice = box_mean(&box_mean(&plane, width, height, 3), width, height, 3);
        let error = out
            .iter()
            .zip(&twice)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max);
        assert!(error < 1e-4, "error {error}");
    }

    #[test]
    fn a_tiny_regulariser_leaves_a_noisy_plane_almost_unchanged() {
        let (width, height) = (64, 48);
        let mut next = stream(6);
        let plane: Vec<f32> = (0..width * height)
            .map(|_| 0.5 + gauss(&mut next, 0.02))
            .collect();
        let out = filtered(&plane, width, height, 3, 1e-10);
        let worst = (6..height - 6)
            .flat_map(|y| (6..width - 6).map(move |x| y * width + x))
            .map(|i| (out[i] - plane[i]).abs())
            .fold(0.0, f32::max);
        eprintln!("tiny eps: worst difference {worst}");
        // Noise of 0.02 (peaks near 0.07) is large against sqrt(1e-10); the interior moves by
        // less than 0.01.
        assert!(worst < 1e-2, "worst difference {worst}");
    }

    /// A vertical step from 0.3 to 0.7 at column `edge`, plus noise of `sigma`.
    fn step(width: usize, height: usize, edge: usize, sigma: f32, seed: u32) -> Vec<f32> {
        let mut next = stream(seed);
        (0..width * height)
            .map(|i| {
                let level = if i % width < edge { 0.3 } else { 0.7 };
                level + gauss(&mut next, sigma)
            })
            .collect()
    }

    fn column_means(plane: &[f32], width: usize, height: usize) -> Vec<f32> {
        (0..width)
            .map(|x| (0..height).map(|y| plane[y * width + x]).sum::<f32>() / height as f32)
            .collect()
    }

    #[test]
    fn a_step_edge_keeps_its_step_while_the_noise_on_the_flat_sides_drops() {
        let (width, height, edge) = (80, 60, 40);
        let plane = step(width, height, edge, 0.02, 9);
        // A regulariser of 0.04^2: above the variance of the noise (4e-4) and well below that of
        // a window that straddles the step (0.04 at the edge).
        let out = filtered(&plane, width, height, 3, 1.6e-3);
        let after = column_means(&out, width, height);
        // The level on each side, four columns or more from the edge, moves by less than 0.01.
        for x in (5..edge - 4).chain(edge + 4..width - 5) {
            let want = if x < edge { 0.3 } else { 0.7 };
            assert!((after[x] - want).abs() < 0.01, "column {x}: {}", after[x]);
        }
        // The transition is no wider than 4 columns: the last column within 0.05 of the low level
        // and the first within 0.05 of the high level are at most 4 apart (a step is 1 apart).
        let low = (0..edge).rev().find(|x| (after[*x] - 0.3).abs() < 0.05);
        let high = (edge..width).find(|x| (after[*x] - 0.7).abs() < 0.05);
        let (low, high) = (low.unwrap(), high.unwrap());
        assert!(high - low <= 4, "transition from column {low} to {high}");
        // The noise on the flat sides, away from the edge, drops by at least a factor of 3.
        let spread = |plane: &[f32], x0: usize, x1: usize, level: f32| {
            let values: Vec<f32> = (0..height)
                .flat_map(|y| (x0..x1).map(move |x| (y, x)))
                .map(|(y, x)| plane[y * width + x] - level)
                .collect();
            (values.iter().map(|v| v * v).sum::<f32>() / values.len() as f32).sqrt()
        };
        for (x0, x1, level) in [(8, edge - 8, 0.3), (edge + 8, width - 8, 0.7)] {
            let (noisy, clean) = (spread(&plane, x0, x1, level), spread(&out, x0, x1, level));
            eprintln!("step edge: noise {noisy} -> {clean}");
            assert!(clean * 3.0 < noisy, "{clean} against {noisy}");
        }
    }

    #[test]
    fn the_guided_filter_is_the_same_on_one_thread_as_on_many() {
        let (width, height) = (97, 61);
        let guide = random_plane(width, height, 21);
        let plane = random_plane(width, height, 22);
        let run = || guided_filter(&guide, &[&plane, &guide], width, height, 5, 1e-2);
        assert_eq!(on_one_thread(run), run());
    }

    fn flat(width: u32, height: u32, rgb: [f32; 3]) -> ImageF32 {
        let data = (0..width * height).flat_map(|_| rgb).collect();
        ImageF32::new(width, height, 3, data).unwrap()
    }

    fn noise(luminance: f32, color: f32) -> Noise {
        Noise { luminance, color }
    }

    /// The Oklab planes of an image.
    fn planes(image: &ImageF32) -> [Vec<f32>; 3] {
        let mut out = [Vec::new(), Vec::new(), Vec::new()];
        for pixel in image.data.as_chunks::<3>().0 {
            let lab = rgb_to_oklab(*pixel);
            for (plane, value) in out.iter_mut().zip(lab) {
                plane.push(value);
            }
        }
        out
    }

    fn mean_and_deviation(values: &[f32]) -> (f32, f32) {
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32;
        (mean, variance.sqrt())
    }

    /// A grey patch with the same noise added to the three channels.
    fn luma_noise_patch(sigma: f32) -> ImageF32 {
        let mut image = flat(96, 72, [0.3; 3]);
        let mut next = stream(31);
        for pixel in image.data.as_chunks_mut::<3>().0 {
            let n = gauss(&mut next, sigma);
            pixel.iter_mut().for_each(|c| *c += n);
        }
        image
    }

    /// A grey patch with independent noise on each channel.
    fn colour_noise_patch(sigma: f32) -> ImageF32 {
        let mut image = flat(96, 72, [0.3; 3]);
        let mut next = stream(32);
        for c in image.data.iter_mut() {
            *c += gauss(&mut next, sigma);
        }
        image
    }

    #[test]
    fn both_amounts_at_zero_leave_the_image_bit_for_bit() {
        let mut image = luma_noise_patch(0.02);
        image.data[10] = f32::NAN;
        image.data[11] = -0.25;
        let before = image.clone();
        let bits = |image: &ImageF32| image.data.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        for scale in [1.0, 0.3] {
            apply_noise_reduction(&mut image, &noise(0.0, 0.0), scale).unwrap();
            assert_eq!(bits(&image), bits(&before));
        }
        // Amounts that are not numbers or negative count as 0.
        apply_noise_reduction(&mut image, &noise(f32::NAN, -5.0), 1.0).unwrap();
        assert_eq!(bits(&image), bits(&before));
    }

    #[test]
    fn luminance_noise_drops_with_the_amount() {
        let patch = luma_noise_patch(0.02);
        let spread = |image: &ImageF32| mean_and_deviation(&planes(image)[0]).1;
        let before = spread(&patch);
        let at = |amount: f32| {
            let mut image = patch.clone();
            apply_noise_reduction(&mut image, &noise(amount, 0.0), 1.0).unwrap();
            spread(&image)
        };
        let (weak, strong) = (at(30.0), at(100.0));
        eprintln!("luma noise: {before} -> {weak} at 30, {strong} at 100");
        assert!(strong * 5.0 < before, "{strong} against {before}");
        assert!(weak < before * 0.8, "{weak} against {before}");
        assert!(strong < weak, "monotonic: {strong} against {weak}");
    }

    #[test]
    fn colour_noise_drops_and_the_mean_colour_stays() {
        let patch = colour_noise_patch(0.02);
        let chroma_spread = |image: &ImageF32| {
            let [_, a, b] = planes(image);
            let (ma, sa) = mean_and_deviation(&a);
            let (mb, sb) = mean_and_deviation(&b);
            ((sa * sa + sb * sb).sqrt(), ma, mb)
        };
        let (before, ma, mb) = chroma_spread(&patch);
        let mut image = patch.clone();
        apply_noise_reduction(&mut image, &noise(0.0, 100.0), 1.0).unwrap();
        let (after, na, nb) = chroma_spread(&image);
        eprintln!("chroma spread: {before} -> {after}");
        assert!(after * 5.0 < before, "{after} against {before}");
        assert!((ma - na).abs() < 2e-3 && (mb - nb).abs() < 2e-3);
        let mut half = patch.clone();
        apply_noise_reduction(&mut half, &noise(0.0, 30.0), 1.0).unwrap();
        let (middle, _, _) = chroma_spread(&half);
        assert!(
            after < middle && middle < before,
            "{after} {middle} {before}"
        );
    }

    #[test]
    fn each_amount_alone_leaves_the_other_channels_alone() {
        let patch = colour_noise_patch(0.02);
        let before = planes(&patch);
        let worst = |after: &[Vec<f32>; 3], plane: usize| {
            before[plane]
                .iter()
                .zip(&after[plane])
                .map(|(x, y)| (x - y).abs())
                .fold(0.0, f32::max)
        };
        // The colour amount alone leaves the lightness within float tolerance.
        let mut image = patch.clone();
        apply_noise_reduction(&mut image, &noise(0.0, 100.0), 1.0).unwrap();
        let after = planes(&image);
        assert!(worst(&after, 0) < 1e-4, "lightness {}", worst(&after, 0));
        // The luminance amount alone leaves the colour within float tolerance.
        let mut image = patch.clone();
        apply_noise_reduction(&mut image, &noise(100.0, 0.0), 1.0).unwrap();
        let after = planes(&image);
        for plane in 1..3 {
            assert!(worst(&after, plane) < 1e-4, "plane {plane}");
        }
    }

    #[test]
    fn a_sharp_colour_edge_stays_in_place_at_the_full_colour_amount() {
        let (width, height, edge) = (96_usize, 48_usize, 48_usize);
        let (left, right) = ([0.6, 0.2, 0.1], [0.05, 0.1, 0.3]);
        let mut image = flat(width as u32, height as u32, left);
        for (i, pixel) in image.data.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            if i % width >= edge {
                pixel.copy_from_slice(&right);
            }
        }
        let before = planes(&image);
        apply_noise_reduction(&mut image, &noise(0.0, 100.0), 1.0).unwrap();
        let after = planes(&image);
        let step_a = (before[1][0] - before[1][width - 1]).abs();
        assert!(step_a > 0.1, "the picture has a colour step");
        // Three or more pixels from the edge, the colour is within 15 percent of the step (the
        // worst change measured is 13 percent, 0.0245, at the second pixel).
        let tolerance = 0.15 * step_a;
        let mut worst = 0.0_f32;
        for y in 0..height {
            for x in (0..width).filter(|x| *x + 3 <= edge || *x >= edge + 3) {
                for plane in 1..3 {
                    let i = y * width + x;
                    worst = worst.max((after[plane][i] - before[plane][i]).abs());
                }
            }
        }
        eprintln!("edge: step {step_a}, worst change {worst}");
        assert!(worst <= tolerance, "worst {worst} against {tolerance}");
    }

    /// A smooth picture with a soft colour pattern, `size` wide, plus noise that has the same
    /// standard deviation per pixel whatever the size.
    fn picture(size: usize, sigma: f32, seed: u32) -> ImageF32 {
        let (width, height) = (size, size * 3 / 4);
        let mut next = stream(seed);
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let (u, v) = (x as f32 / width as f32, y as f32 / height as f32);
                let base = [
                    0.25 + 0.2 * u,
                    0.3 + 0.15 * v + 0.05 * (6.0 * u).sin(),
                    0.2 + 0.1 * (u + v),
                ];
                for c in base {
                    data.push(c + gauss(&mut next, sigma));
                }
            }
        }
        ImageF32::new(width as u32, height as u32, 3, data).unwrap()
    }

    fn halved(image: &ImageF32) -> ImageF32 {
        let (w, h) = (image.width as usize, image.height as usize);
        let mut data = Vec::new();
        for y in 0..h / 2 {
            for x in 0..w / 2 {
                for c in 0..3 {
                    let at =
                        |dy: usize, dx: usize| image.data[((2 * y + dy) * w + 2 * x + dx) * 3 + c];
                    data.push((at(0, 0) + at(0, 1) + at(1, 0) + at(1, 1)) / 4.0);
                }
            }
        }
        ImageF32::new((w / 2) as u32, (h / 2) as u32, 3, data).unwrap()
    }

    fn mean_difference(a: &ImageF32, b: &ImageF32) -> f32 {
        a.data
            .iter()
            .zip(&b.data)
            .map(|(x, y)| (x - y).abs())
            .sum::<f32>()
            / a.data.len() as f32
    }

    #[test]
    fn a_preview_at_half_size_agrees_with_the_full_size_result() {
        let full = picture(192, 0.02, 41);
        let mut full_result = full.clone();
        apply_noise_reduction(&mut full_result, &noise(60.0, 60.0), 1.0).unwrap();
        let reference = halved(&full_result);
        // The preview: the full picture downscaled first, then reduced with the scale 0.5.
        let mut small = halved(&full);
        let untouched = small.clone();
        apply_noise_reduction(&mut small, &noise(60.0, 60.0), 0.5).unwrap();
        let error = mean_difference(&reference, &small);
        let change = mean_difference(&reference, &untouched);
        eprintln!("scale: mean difference {error}, the filter changes the picture by {change}");
        // The mean difference between the two is below 0.004 per sample (the noise is 0.02 per
        // sample at full size and 0.01 after halving).
        assert!(error < 0.004, "mean difference {error}");
    }

    #[test]
    fn non_finite_pixels_are_kept_and_do_not_spread() {
        let width = 64_usize;
        let mut image = picture(64, 0.02, 51);
        let bad = [(5_usize, 5_usize), (6, 5), (30, 40)];
        for (k, (x, y)) in bad.iter().enumerate() {
            image.data[(y * width + x) * 3] = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY][k];
        }
        let before = image.clone();
        apply_noise_reduction(&mut image, &noise(80.0, 80.0), 1.0).unwrap();
        let bad_pixels: Vec<usize> = bad.iter().map(|(x, y)| y * width + x).collect();
        for (i, pixel) in image.data.as_chunks::<3>().0.iter().enumerate() {
            if bad_pixels.contains(&i) {
                for (c, value) in pixel.iter().enumerate() {
                    assert_eq!(value.to_bits(), before.data[i * 3 + c].to_bits());
                }
            } else {
                assert!(pixel.iter().all(|c| c.is_finite()), "pixel {i}: {pixel:?}");
            }
        }
    }

    #[test]
    fn the_result_does_not_depend_on_the_thread_count() {
        let image = picture(120, 0.03, 61);
        let make = || {
            let mut image = image.clone();
            apply_noise_reduction(&mut image, &noise(70.0, 50.0), 0.8).unwrap();
            image
        };
        assert_eq!(on_one_thread(make), make());
    }

    #[test]
    fn a_scale_that_is_not_positive_and_a_grey_image_are_invalid() {
        let mut image = flat(8, 8, [0.3; 3]);
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let err = apply_noise_reduction(&mut image, &noise(50.0, 50.0), scale).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
        }
        let mut gray = ImageF32::zeros(4, 4, 1).unwrap();
        let err = apply_noise_reduction(&mut gray, &noise(50.0, 50.0), 1.0).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn radii_are_whole_pixels_and_at_least_one() {
        assert_eq!(scaled_radius(NR_LUMA_RADIUS_PX, 1.0), 3);
        assert_eq!(scaled_radius(NR_CHROMA_RADIUS_PX, 1.0), 8);
        assert_eq!(scaled_radius(NR_LUMA_RADIUS_PX, 0.5), 2);
        assert_eq!(scaled_radius(NR_CHROMA_RADIUS_PX, 0.5), 4);
        assert_eq!(scaled_radius(NR_LUMA_RADIUS_PX, 0.01), 1);
    }

    /// Cost of the stage on a 6000 x 4000 image; run with
    /// `cargo test -p gtp-edit --release -- --ignored noise_timing --nocapture`.
    #[test]
    #[ignore = "timing only"]
    fn noise_timing_on_a_6000_by_4000_image() {
        let mut next = stream(77);
        let mut image = flat(6000, 4000, [0.0; 3]);
        for c in image.data.iter_mut() {
            *c = 0.3 + gauss(&mut next, 0.02);
        }
        let start = std::time::Instant::now();
        apply_noise_reduction(&mut image, &noise(50.0, 50.0), 1.0).unwrap();
        eprintln!(
            "noise reduction of 6000 x 4000: {:?} on {} threads",
            start.elapsed(),
            rayon::current_num_threads()
        );
    }

    // ---- Frozen copies of the code before P-31 phase 2 (a full-size buffer per step). The new code
    // ---- must give the same bits as these.

    /// `sum_rows` before P-31.
    fn sum_rows_before_p31(src: &[f32], width: usize, height: usize, r: usize) -> Vec<f32> {
        let mut out = vec![0.0_f32; width * height];
        if width == 0 || height == 0 {
            return out;
        }
        out.par_chunks_mut(width)
            .zip(src.par_chunks(width))
            .for_each(|(dst, row)| {
                let mut acc: f64 = row[..=r.min(width - 1)].iter().map(|v| *v as f64).sum();
                for x in 0..width {
                    dst[x] = acc as f32;
                    if x + 1 + r < width {
                        acc += row[x + 1 + r] as f64;
                    }
                    if x >= r {
                        acc -= row[x - r] as f64;
                    }
                }
            });
        out
    }

    /// `mean_of_row_sums` before P-31.
    fn mean_of_row_sums_before_p31(
        sums: &[f32],
        width: usize,
        height: usize,
        r: usize,
        across: &[f32],
    ) -> Vec<f32> {
        let mut out = vec![0.0_f32; sums.len()];
        let down = window_counts(height, r);
        out.par_chunks_mut(COLUMN_BAND * width)
            .enumerate()
            .for_each(|(band, dst)| {
                let first = band * COLUMN_BAND;
                let mut acc = vec![0.0_f64; width];
                for row in first.saturating_sub(r)..=(first + r).min(height - 1) {
                    for (a, v) in acc.iter_mut().zip(&sums[row * width..(row + 1) * width]) {
                        *a += *v as f64;
                    }
                }
                for (i, line) in dst.chunks_mut(width).enumerate() {
                    let y = first + i;
                    for ((o, a), n) in line.iter_mut().zip(&acc).zip(across) {
                        *o = (*a as f32) / (n * down[y]);
                    }
                    if y + 1 + r < height {
                        let add = &sums[(y + 1 + r) * width..(y + 2 + r) * width];
                        for (a, v) in acc.iter_mut().zip(add) {
                            *a += *v as f64;
                        }
                    }
                    if y >= r {
                        let sub = &sums[(y - r) * width..(y - r + 1) * width];
                        for (a, v) in acc.iter_mut().zip(sub) {
                            *a -= *v as f64;
                        }
                    }
                }
            });
        out
    }

    /// `box_mean` before P-31.
    fn box_mean_before_p31(src: &[f32], width: usize, height: usize, r: usize) -> Vec<f32> {
        if width == 0 || height == 0 {
            return Vec::new();
        }
        let rows = sum_rows_before_p31(src, width, height, r);
        mean_of_row_sums_before_p31(&rows, width, height, r, &window_counts(width, r))
    }

    /// `zip_map` before P-31.
    fn zip_map_before_p31(a: &[f32], b: &[f32], f: impl Fn(f32, f32) -> f32 + Sync) -> Vec<f32> {
        a.par_iter()
            .zip(b.par_iter())
            .map(|(x, y)| f(*x, *y))
            .collect()
    }

    /// `guided_filter` before P-31.
    fn guided_filter_before_p31(
        guide: &[f32],
        planes: &[&[f32]],
        width: usize,
        height: usize,
        r: usize,
        eps: f32,
    ) -> Vec<Vec<f32>> {
        let shift_guide = plane_mean(guide);
        let centred_guide: Vec<f32> = guide.par_iter().map(|v| v - shift_guide).collect();
        let mean_guide = box_mean_before_p31(&centred_guide, width, height, r);
        let squares = zip_map_before_p31(&centred_guide, &centred_guide, |x, y| x * y);
        let mean_squares = box_mean_before_p31(&squares, width, height, r);
        drop(squares);
        let variance =
            zip_map_before_p31(&mean_squares, &mean_guide, |ii, i| (ii - i * i).max(0.0));

        planes
            .iter()
            .map(|plane| {
                let own = std::ptr::eq(plane.as_ptr(), guide.as_ptr());
                let shift = if own { shift_guide } else { plane_mean(plane) };
                let centred: Vec<f32>;
                let p: &[f32] = if own {
                    &centred_guide
                } else {
                    centred = plane.par_iter().map(|v| v - shift).collect();
                    &centred
                };
                let mean_p = if own {
                    mean_guide.clone()
                } else {
                    box_mean_before_p31(p, width, height, r)
                };
                let computed;
                let mean_products: &[f32] = if own {
                    &mean_squares
                } else {
                    let products = zip_map_before_p31(&centred_guide, p, |x, y| x * y);
                    computed = box_mean_before_p31(&products, width, height, r);
                    &computed
                };
                let slope: Vec<f32> = (0..mean_p.len())
                    .into_par_iter()
                    .map(|i| {
                        let covariance = mean_products[i] - mean_guide[i] * mean_p[i];
                        let denominator = variance[i] + eps;
                        if denominator > 0.0 {
                            covariance / denominator
                        } else {
                            0.0
                        }
                    })
                    .collect();
                let offset = (0..mean_p.len())
                    .into_par_iter()
                    .map(|i| mean_p[i] - slope[i] * mean_guide[i])
                    .collect::<Vec<f32>>();
                drop(mean_p);
                let mean_slope = box_mean_before_p31(&slope, width, height, r);
                drop(slope);
                let mean_offset = box_mean_before_p31(&offset, width, height, r);
                drop(offset);
                (0..mean_slope.len())
                    .into_par_iter()
                    .map(|i| mean_slope[i] * centred_guide[i] + mean_offset[i] + shift)
                    .collect()
            })
            .collect()
    }

    /// `apply_noise_reduction` before P-31.
    fn apply_noise_reduction_before_p31(
        image: &mut ImageF32,
        noise: &Noise,
        scale: f32,
    ) -> Result<(), AppError> {
        require_rgb(image, "noise reduction")?;
        if !scale.is_finite() || scale <= 0.0 {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "The noise reduction stage needs a positive scale.",
            )
            .with_details(scale.to_string()));
        }
        let (luminance, color) = (limited(noise.luminance), limited(noise.color));
        if (luminance == 0.0 && color == 0.0) || image.data.is_empty() {
            return Ok(());
        }
        let (width, height) = (image.width as usize, image.height as usize);
        let count = width * height;
        let (mut l, mut a, mut b) = (vec![0.0; count], vec![0.0; count], vec![0.0; count]);
        l.par_chunks_mut(width)
            .zip(a.par_chunks_mut(width))
            .zip(b.par_chunks_mut(width))
            .zip(image.data.par_chunks(width * 3))
            .for_each(|(((l, a), b), row)| {
                for (i, pixel) in row.as_chunks::<3>().0.iter().enumerate() {
                    if pixel.iter().all(|c| c.is_finite()) {
                        [l[i], a[i], b[i]] = rgb_to_oklab(*pixel);
                    }
                }
            });
        if luminance > 0.0 {
            let r = scaled_radius(NR_LUMA_RADIUS_PX, scale);
            let eps = regulariser(luminance, NR_LUMA_MAX_SIGMA, scale);
            let mut out = guided_filter_before_p31(&l, &[&l], width, height, r, eps);
            l = out.swap_remove(0);
        }
        if color > 0.0 {
            let r = scaled_radius(NR_CHROMA_RADIUS_PX, scale);
            let eps = regulariser(color, NR_CHROMA_MAX_SIGMA, scale);
            let mut out = guided_filter_before_p31(&l, &[&a, &b], width, height, r, eps);
            b = out.swap_remove(1);
            a = out.swap_remove(0);
        }
        image
            .data
            .par_chunks_mut(width * 3)
            .zip(l.par_chunks(width))
            .zip(a.par_chunks(width))
            .zip(b.par_chunks(width))
            .for_each(|(((row, l), a), b)| {
                for (i, pixel) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                    if !pixel.iter().all(|c| c.is_finite()) {
                        continue;
                    }
                    let out = oklab_to_rgb([l[i], a[i], b[i]]);
                    if out.iter().all(|c| c.is_finite()) {
                        *pixel = out;
                    }
                }
            });
        Ok(())
    }

    // ---- Bit-for-bit comparison of the new code with the frozen copies.

    /// A plane with negative values, zeros and large values mixed in.
    fn wild_plane(width: usize, height: usize, seed: u32) -> Vec<f32> {
        let mut next = stream(seed);
        (0..width * height)
            .map(|i| {
                let v = next() - 0.5;
                match i % 11 {
                    3 => 0.0,
                    7 => v * 40_000.0,
                    _ => v * 3.0,
                }
            })
            .collect()
    }

    fn assert_same_bits(new: &[f32], old: &[f32], what: &str) {
        assert_eq!(new.len(), old.len(), "{what}: length");
        for (i, (n, o)) in new.iter().zip(old).enumerate() {
            assert_eq!(
                n.to_bits(),
                o.to_bits(),
                "{what}: sample {i}: {n} against {o}"
            );
        }
    }

    /// Sizes that cover heights below, at and above the band of 64 rows, widths below `2r + 1`,
    /// and single rows and columns.
    const BIT_SIZES: [(usize, usize); 11] = [
        (1, 1),
        (1, 9),
        (9, 1),
        (5, 3),
        (13, 70),
        (70, 13),
        (37, 129),
        (100, 64),
        (64, 100),
        (21, 65),
        (130, 200),
    ];

    fn pool(threads: usize) -> rayon::ThreadPool {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
    }

    #[test]
    fn the_box_average_has_the_same_bits_as_before_p31() {
        for threads in [1, 6] {
            pool(threads).install(|| {
                for (n, &(width, height)) in BIT_SIZES.iter().enumerate() {
                    let plane = wild_plane(width, height, 500 + n as u32);
                    for r in [1, 3, 8, 60] {
                        assert_same_bits(
                            &box_mean(&plane, width, height, r),
                            &box_mean_before_p31(&plane, width, height, r),
                            &format!("{width}x{height}, r {r}, {threads} threads"),
                        );
                    }
                }
            });
        }
    }

    #[test]
    fn the_guided_filter_has_the_same_bits_as_before_p31() {
        for threads in [1, 6] {
            pool(threads).install(|| {
                for (n, &(width, height)) in BIT_SIZES.iter().enumerate() {
                    let guide = wild_plane(width, height, 600 + n as u32);
                    let a = wild_plane(width, height, 700 + n as u32);
                    let constant = vec![0.25_f32; width * height];
                    for (r, eps) in [(1, 0.01), (3, 0.0), (8, 1e-4), (60, 5.0)] {
                        let planes: [&[f32]; 4] = [&guide, &a, &constant, &guide];
                        let new = guided_filter(&guide, &planes, width, height, r, eps);
                        let old = guided_filter_before_p31(&guide, &planes, width, height, r, eps);
                        assert_eq!(new.len(), old.len());
                        for (k, (n, o)) in new.iter().zip(&old).enumerate() {
                            assert_same_bits(
                                n,
                                o,
                                &format!("{width}x{height}, r {r}, plane {k}, {threads} threads"),
                            );
                        }
                    }
                    // A constant guide leaves nothing but the regulariser in the denominator.
                    let new = guided_filter(&constant, &[&a], width, height, 3, 0.0);
                    let old = guided_filter_before_p31(&constant, &[&a], width, height, 3, 0.0);
                    assert_same_bits(&new[0], &old[0], &format!("{width}x{height}, flat guide"));
                }
            });
        }
    }

    #[test]
    fn a_reused_workspace_gives_the_same_bits_as_a_fresh_one() {
        let (width, height) = (70, 90);
        let guide = wild_plane(width, height, 31);
        let a = wild_plane(width, height, 32);
        let b = wild_plane(width, height, 33);
        let mut ws = Workspace::default();
        for (r, eps) in [(3, 0.01), (8, 0.5), (3, 0.01)] {
            let reused = guided_filter_in(&mut ws, &guide, &[&a, &b], width, height, r, eps);
            let old = guided_filter_before_p31(&guide, &[&a, &b], width, height, r, eps);
            for (n, o) in reused.iter().zip(&old) {
                assert_same_bits(n, o, "reused workspace");
            }
            for plane in reused {
                ws.give(plane);
            }
        }
    }

    fn wild_image(width: usize, height: usize, seed: u32, non_finite: bool) -> ImageF32 {
        let mut image = flat(width as u32, height as u32, [0.0; 3]);
        image.data = wild_plane(width * 3, height, seed)
            .iter()
            .map(|v| v / 20_000.0 + 0.4)
            .collect();
        if non_finite {
            let len = image.data.len();
            for (k, bad) in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY]
                .iter()
                .enumerate()
            {
                if len > 0 {
                    image.data[(k * 37 + 1) % len] = *bad;
                }
            }
        }
        image
    }

    #[test]
    fn noise_reduction_has_the_same_bits_as_before_p31() {
        let amounts = [
            (50.0, 0.0),
            (0.0, 50.0),
            (70.0, 40.0),
            (100.0, 100.0),
            (0.0, 0.0),
        ];
        for threads in [1, 6] {
            pool(threads).install(|| {
                for (n, &(width, height)) in [(1, 1), (7, 5), (40, 70), (97, 67)].iter().enumerate()
                {
                    for non_finite in [false, true] {
                        let image = wild_image(width, height, 800 + n as u32, non_finite);
                        for &(luminance, color) in &amounts {
                            for scale in [1.0, 0.5, 2.5] {
                                let settings = noise(luminance, color);
                                let mut new = image.clone();
                                let mut old = image.clone();
                                apply_noise_reduction(&mut new, &settings, scale).unwrap();
                                apply_noise_reduction_before_p31(&mut old, &settings, scale)
                                    .unwrap();
                                assert_same_bits(
                                    &new.data,
                                    &old.data,
                                    &format!(
                                        "{width}x{height}, amounts {luminance}/{color}, scale \
                                         {scale}, non-finite {non_finite}, {threads} threads"
                                    ),
                                );
                            }
                        }
                    }
                }
            });
        }
    }
}
