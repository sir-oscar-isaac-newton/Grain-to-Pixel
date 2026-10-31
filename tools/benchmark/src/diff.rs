//! Per-pixel CIEDE2000 between two renders, and its summary.

use gtp_core::{AppError, ErrorCode, ImageF32};

use crate::delta_e::{delta_e2000, rec2020_to_lab};

/// Summary of the per-pixel ΔE2000 between two images.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageDiff {
    pub mean: f32,
    /// 95th percentile (nearest rank).
    pub p95: f32,
    pub max: f32,
}

/// The ΔE2000 of every pixel of two linear Rec.2020 images of the same size, as a 1-channel image.
pub fn delta_e_map(a: &ImageF32, b: &ImageF32) -> Result<ImageF32, AppError> {
    let same_shape = a.width == b.width && a.height == b.height && a.channels == b.channels;
    if !same_shape || a.channels != 3 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Only two 3-channel images of the same size can be compared.",
        )
        .with_details(format!(
            "{}x{}x{} vs {}x{}x{}",
            a.width, a.height, a.channels, b.width, b.height, b.channels
        )));
    }
    let (pa, _) = a.data.as_chunks::<3>();
    let (pb, _) = b.data.as_chunks::<3>();
    let data = pa
        .iter()
        .zip(pb)
        .map(|(x, y)| delta_e2000(rec2020_to_lab(*x), rec2020_to_lab(*y)))
        .collect();
    ImageF32::new(a.width, a.height, 1, data)
}

impl ImageDiff {
    /// Summarizes a ΔE map; an empty map is all zeros.
    pub fn of_map(map: &ImageF32) -> Self {
        let mut values = map.data.clone();
        if values.is_empty() {
            return Self {
                mean: 0.0,
                p95: 0.0,
                max: 0.0,
            };
        }
        values.sort_unstable_by(f32::total_cmp);
        let n = values.len();
        let sum: f64 = values.iter().map(|v| f64::from(*v)).sum();
        let rank = (n * 95).div_ceil(100).max(1) - 1;
        Self {
            mean: (sum / n as f64) as f32,
            p95: values[rank],
            max: values[n - 1],
        }
    }

    /// Compares two images (see [`delta_e_map`]).
    pub fn between(a: &ImageF32, b: &ImageF32) -> Result<Self, AppError> {
        Ok(Self::of_map(&delta_e_map(a, b)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(pixels: &[[f32; 3]]) -> ImageF32 {
        ImageF32::new(pixels.len() as u32, 1, 3, pixels.concat()).unwrap()
    }

    #[test]
    fn identical_images_differ_by_zero() {
        let img = image(&[[0.1, 0.2, 0.3], [0.9, 0.5, 0.1]]);
        let d = ImageDiff::between(&img, &img).unwrap();
        assert_eq!((d.mean, d.p95, d.max), (0.0, 0.0, 0.0));
    }

    #[test]
    fn one_changed_pixel_shows_in_max_but_not_p95() {
        let grey = [0.18; 3];
        let a = image(&[grey; 100]);
        let mut changed = [grey; 100];
        changed[42] = [0.36, 0.18, 0.18];
        let b = image(&changed);
        let d = ImageDiff::between(&a, &b).unwrap();
        let pixel = delta_e2000(rec2020_to_lab(grey), rec2020_to_lab(changed[42]));
        assert!(pixel > 5.0);
        assert!((d.max - pixel).abs() < 1e-6);
        assert_eq!(d.p95, 0.0);
        assert!((d.mean - pixel / 100.0).abs() < 1e-5);
    }

    #[test]
    fn the_percentile_is_the_nearest_rank() {
        let map = ImageF32::new(20, 1, 1, (1..=20).map(|v| v as f32).collect()).unwrap();
        let d = ImageDiff::of_map(&map);
        assert_eq!((d.p95, d.max), (19.0, 20.0));
        assert!((d.mean - 10.5).abs() < 1e-6);
    }

    #[test]
    fn different_sizes_are_rejected() {
        let err = ImageDiff::between(&image(&[[0.0; 3]]), &image(&[[0.0; 3]; 2])).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }
}
