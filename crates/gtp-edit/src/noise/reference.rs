//! A plain, slow version of the luminance wavelet, to check the tiled one against: whole planes,
//! mirrored borders at every scale, one smoothing after the other. Test code only.

use super::tests::{gauss_for_measure, stream_for_measure};
use super::*;

/// Border left out of the energies, in pixels.
pub(super) const ENERGY_MARGIN: usize = 64;

/// `src` smoothed along the rows and then the columns with the B3 spline kernel whose taps are `d`
/// samples apart, mirrored at the borders, straight from the definition.
pub(super) fn smooth_level(src: &[f32], dst: &mut [f32], width: usize, height: usize, d: usize) {
    let kernel: Vec<(isize, f32)> = (-2..=2).zip(B3_SPLINE_TAPS).collect();
    let mut rows = vec![0.0_f32; src.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            for &(t, k) in &kernel {
                let sx = mirrored(x as isize + t * d as isize, width);
                sum += k * src[y * width + sx];
            }
            rows[y * width + x] = sum / B3_SPLINE_NORM;
        }
    }
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            for &(t, k) in &kernel {
                let sy = mirrored(y as isize + t * d as isize, height);
                sum += k * rows[sy * width + x];
            }
            dst[y * width + x] = sum / B3_SPLINE_NORM;
        }
    }
}

/// The luminance wavelet of `l` computed plane by plane, with the noise levels taken from the same
/// patches as the real one.
pub(super) fn wavelet_reference(
    l: &[f32],
    (width, height): (usize, usize),
    amount: f32,
    scale: f32,
    strength: f32,
) -> Vec<f32> {
    let (first, levels) = wavelet_plan(scale);
    let mut current = l.to_vec();
    let mut details = Vec::new();
    for level in 0..levels {
        let mut next = vec![0.0; l.len()];
        smooth_level(&current, &mut next, width, height, 1 << level);
        details.push(
            current
                .iter()
                .zip(&next)
                .map(|(c, n)| c - n)
                .collect::<Vec<f32>>(),
        );
        current = next;
    }
    let patches = wavelet_patches(width, height);
    let mut result = current;
    for (level, detail) in details.iter().enumerate() {
        let mut values = Vec::new();
        for &(x0, y0, pw, ph) in &patches {
            for y in y0..y0 + ph {
                for x in x0..x0 + pw {
                    values.push(detail[y * width + x].abs());
                }
            }
        }
        let sigma = sigma_from_absolute_values(values);
        let threshold =
            amount / NOISE_AMOUNT_MAX * strength * NR_WAVELET_SHARES[first + level] * sigma;
        for (r, d) in result.iter_mut().zip(detail) {
            *r += shrunk(*d, threshold);
        }
    }
    result
}

/// The energy of each of the scales 0 to 4 of `l`.
pub(super) fn energies(l: &[f32], width: usize, height: usize) -> [f64; 5] {
    let mut current = l.to_vec();
    let mut next = vec![0.0; l.len()];
    let mut out = [0.0; 5];
    for (j, e) in out.iter_mut().enumerate() {
        smooth_level(&current, &mut next, width, height, 1 << j);
        let mut sum = 0.0_f64;
        let mut n = 0_usize;
        for y in ENERGY_MARGIN..height - ENERGY_MARGIN {
            for x in ENERGY_MARGIN..width - ENERGY_MARGIN {
                let d = (current[y * width + x] - next[y * width + x]) as f64;
                sum += d * d;
                n += 1;
            }
        }
        *e = sum / n as f64;
        std::mem::swap(&mut current, &mut next);
    }
    out
}

/// Energy of the coarse scales (2 to 4) together.
pub(super) fn coarse(e: &[f64; 5]) -> f64 {
    e[2] + e[3] + e[4]
}

/// A seeded correlated noise: white noise plus the same smoothed by the a trous cascade (dilations
/// 1, 2, 4, ...) through `n` levels, for each `n` of `cascades`, each layer brought to a standard
/// deviation of `sigma`. A cascade of `n` levels leaves blotches of about `2^n` pixels.
pub(super) fn grain(
    width: usize,
    height: usize,
    seed: u32,
    sigma: f32,
    cascades: &[usize],
) -> Vec<f32> {
    let mut next = stream_for_measure(seed);
    let mut total = vec![0.0_f32; width * height];
    let mut layers = vec![0_usize];
    layers.extend_from_slice(cascades);
    for levels in layers {
        let mut layer: Vec<f32> = (0..width * height)
            .map(|_| gauss_for_measure(&mut next, 1.0))
            .collect();
        for j in 0..levels {
            let mut smooth = vec![0.0; width * height];
            smooth_level(&layer, &mut smooth, width, height, 1 << j);
            layer = smooth;
        }
        let mean = layer.iter().sum::<f32>() / layer.len() as f32;
        let deviation =
            (layer.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / layer.len() as f32).sqrt();
        for (t, v) in total.iter_mut().zip(&layer) {
            *t += (v - mean) / deviation * sigma;
        }
    }
    total
}
