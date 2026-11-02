//! Registration of a reference render inside our render of the whole capture (Q-02).
//!
//! The reference is a crop of the same picture, at another size and in other tones, so the search
//! is over the crop only: its position and its scale, with the reference's aspect ratio. Matching
//! uses the normalized cross-correlation of the brightness, which does not care how the tones
//! differ (a washed-out render against a contrasty one), and goes from a coarse grid to a fine one.

use gtp_core::{AppError, ErrorCode, ImageF32};

/// A grey image.
#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
    pub w: usize,
    pub h: usize,
    pub data: Vec<f32>,
}

/// A rectangle as fractions of an image: left, top, width and height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Where the reference sits in the render, and how well the two agree there (-1 to 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Registration {
    pub window: Window,
    pub score: f32,
}

impl Plane {
    fn at(&self, x: usize, y: usize) -> f32 {
        self.data[y.min(self.h - 1) * self.w + x.min(self.w - 1)]
    }

    /// Bilinear sample at a position in pixel units (pixel centres at +0.5), clamped at the edges.
    fn sample(&self, x: f32, y: f32) -> f32 {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let clamp = |v: f32, max: usize| (v.max(0.0) as usize).min(max - 1);
        let (xa, ya) = (clamp(x0, self.w), clamp(y0, self.h));
        let (xb, yb) = (clamp(x0 + 1.0, self.w), clamp(y0 + 1.0, self.h));
        let top = self.at(xa, ya) * (1.0 - tx) + self.at(xb, ya) * tx;
        let bottom = self.at(xa, yb) * (1.0 - tx) + self.at(xb, yb) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

/// The mean of the three channels of every pixel.
pub fn luma(image: &ImageF32) -> Plane {
    let (pixels, _) = image.data.as_chunks::<3>();
    Plane {
        w: image.width as usize,
        h: image.height as usize,
        data: pixels.iter().map(|p| (p[0] + p[1] + p[2]) / 3.0).collect(),
    }
}

/// The plane shrunk to `out_w` columns by averaging the pixels each new one covers; never enlarged.
pub fn box_downscale(plane: &Plane, out_w: usize) -> Plane {
    let out_w = out_w.clamp(1, plane.w);
    let out_h = ((plane.h * out_w) as f32 / plane.w as f32).round().max(1.0) as usize;
    let mut data = Vec::with_capacity(out_w * out_h);
    for oy in 0..out_h {
        let (y0, y1) = (oy * plane.h / out_h, ((oy + 1) * plane.h).div_ceil(out_h));
        for ox in 0..out_w {
            let (x0, x1) = (ox * plane.w / out_w, ((ox + 1) * plane.w).div_ceil(out_w));
            let mut sum = 0.0f64;
            for y in y0..y1.max(y0 + 1) {
                for x in x0..x1.max(x0 + 1) {
                    sum += f64::from(plane.at(x, y));
                }
            }
            let count = (y1.max(y0 + 1) - y0) * (x1.max(x0 + 1) - x0);
            data.push((sum / count as f64) as f32);
        }
    }
    Plane {
        w: out_w,
        h: out_h,
        data,
    }
}

/// The values of a window of `source` (in its pixel units) sampled on an `out_w` x `out_h` grid.
fn window_samples(
    source: &Plane,
    (x0, y0, ww, wh): (f32, f32, f32, f32),
    (out_w, out_h): (usize, usize),
) -> Vec<f32> {
    let mut values = Vec::with_capacity(out_w * out_h);
    for j in 0..out_h {
        let y = y0 + (j as f32 + 0.5) * wh / out_h as f32;
        for i in 0..out_w {
            values.push(source.sample(x0 + (i as f32 + 0.5) * ww / out_w as f32, y));
        }
    }
    values
}

/// Zero mean and unit length, so a dot product of two is their normalized cross-correlation. `None`
/// for a flat patch, which correlates with nothing.
fn normalized(values: &mut [f32]) -> Option<()> {
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    values.iter_mut().for_each(|v| *v -= mean);
    let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm <= 1e-9 || !norm.is_finite() {
        return None;
    }
    values.iter_mut().for_each(|v| *v /= norm);
    Some(())
}

/// The best window of `source` among those tried, scored against the normalized `reference`.
struct Search<'a> {
    source: &'a Plane,
    reference: &'a Plane,
    wanted: Vec<f32>,
    best: Option<(f32, (f32, f32, f32))>,
}

impl<'a> Search<'a> {
    fn new(source: &'a Plane, reference: &'a Plane) -> Option<Self> {
        let mut wanted = reference.data.clone();
        normalized(&mut wanted)?;
        Some(Self {
            source,
            reference,
            wanted,
            best: None,
        })
    }

    /// Tries the window at (`x0`, `y0`) of width `ww` (its height follows the reference's aspect).
    fn try_window(&mut self, x0: f32, y0: f32, ww: f32) {
        let aspect = self.reference.w as f32 / self.reference.h as f32;
        let wh = ww / aspect;
        let (sw, sh) = (self.source.w as f32, self.source.h as f32);
        if ww < 2.0 || x0 < 0.0 || y0 < 0.0 || x0 + ww > sw + 0.01 || y0 + wh > sh + 0.01 {
            return;
        }
        let mut values = window_samples(
            self.source,
            (x0, y0, ww, wh),
            (self.reference.w, self.reference.h),
        );
        if normalized(&mut values).is_none() {
            return;
        }
        let score: f32 = values.iter().zip(&self.wanted).map(|(a, b)| a * b).sum();
        if self.best.is_none_or(|(best, _)| score > best) {
            self.best = Some((score, (x0, y0, ww)));
        }
    }
}

/// Finds where `reference` sits inside `source`: the window of the reference's aspect ratio that
/// matches it best, at a scale from half to all of the source's width. `None` when either picture is
/// flat or too small. The score of a good match is close to 1.
pub fn register(source: &Plane, reference: &Plane) -> Option<Registration> {
    if source.w < 16 || source.h < 16 || reference.w < 8 || reference.h < 8 {
        return None;
    }
    // Coarse: every position and scale on a small copy.
    let (coarse_src, coarse_ref) = (box_downscale(source, 96), box_downscale(reference, 48));
    let mut coarse = Search::new(&coarse_src, &coarse_ref)?;
    let width = coarse_src.w as f32;
    let mut scale = 0.5f32;
    while scale <= 1.0001 {
        let ww = scale * width;
        let wh = ww / (coarse_ref.w as f32 / coarse_ref.h as f32);
        let (max_x, max_y) = (
            (width - ww).max(0.0) as usize,
            (coarse_src.h as f32 - wh).max(0.0) as usize,
        );
        for y0 in 0..=max_y {
            for x0 in 0..=max_x {
                coarse.try_window(x0 as f32, y0 as f32, ww);
            }
        }
        scale += 0.025;
    }
    let (_, (cx, cy, cw)) = coarse.best?;

    // Fine: around the coarse answer, on a larger copy, in single pixels.
    let (fine_src, fine_ref) = (box_downscale(source, 512), box_downscale(reference, 128));
    let mut fine = Search::new(&fine_src, &fine_ref)?;
    let k = fine_src.w as f32 / coarse_src.w as f32;
    let (fx, fy, fw) = (cx * k, cy * k, cw * k);
    // One coarse pixel is k fine ones, so that is how far the coarse answer can be off.
    for dw in -4..=4 {
        let ww = fw + dw as f32 * 2.0;
        for dy in -7..=7 {
            for dx in -7..=7 {
                fine.try_window(fx + dx as f32, fy + dy as f32, ww);
            }
        }
    }
    let (score, (x0, y0, ww)) = fine.best?;
    let aspect = fine_ref.w as f32 / fine_ref.h as f32;
    let (sw, sh) = (fine_src.w as f32, fine_src.h as f32);
    Some(Registration {
        window: Window {
            x: x0 / sw,
            y: y0 / sh,
            w: ww / sw,
            h: ww / aspect / sh,
        },
        score,
    })
}

/// The part of a 3-channel image in `window`, resampled to `out_w` x `out_h` (bilinear).
pub fn crop_resample(
    image: &ImageF32,
    window: Window,
    out_w: usize,
    out_h: usize,
) -> Result<ImageF32, AppError> {
    if image.channels != 3 || out_w == 0 || out_h == 0 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Only a 3-channel image can be cropped to a size above 0.",
        ));
    }
    let (w, h) = (image.width as usize, image.height as usize);
    let channel = |c: usize| Plane {
        w,
        h,
        data: image.data.iter().skip(c).step_by(3).copied().collect(),
    };
    let rect = (
        window.x * w as f32,
        window.y * h as f32,
        window.w * w as f32,
        window.h * h as f32,
    );
    let planes: Vec<Vec<f32>> = (0..3)
        .map(|c| window_samples(&channel(c), rect, (out_w, out_h)))
        .collect();
    let mut data = Vec::with_capacity(out_w * out_h * 3);
    for ((r, g), b) in planes[0].iter().zip(&planes[1]).zip(&planes[2]) {
        data.extend([*r, *g, *b]);
    }
    ImageF32::new(out_w as u32, out_h as u32, 3, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture with structure at every scale and no symmetry.
    fn picture(w: usize, h: usize) -> Plane {
        let mut data = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
                let blob = (-(((u - 0.35).powi(2) + (v - 0.6).powi(2)) / 0.02)).exp();
                data.push(
                    0.5 + 0.2 * (u * 17.0).sin() * (v * 11.0).cos()
                        + 0.15 * (u * 5.0 + v * 3.0).sin()
                        + 0.4 * blob,
                );
            }
        }
        Plane { w, h, data }
    }

    /// The reference: `window` of the picture at `out_w` columns, with the tones changed.
    fn reference_of(
        source: &Plane,
        window: Window,
        out_w: usize,
        tone: impl Fn(f32) -> f32,
    ) -> Plane {
        let out_h = (window_aspect(source, window)).recip() * out_w as f32;
        let out_h = out_h.round() as usize;
        let rect = (
            window.x * source.w as f32,
            window.y * source.h as f32,
            window.w * source.w as f32,
            window.h * source.h as f32,
        );
        let data = window_samples(source, rect, (out_w, out_h))
            .into_iter()
            .map(tone)
            .collect();
        Plane {
            w: out_w,
            h: out_h,
            data,
        }
    }

    fn window_aspect(source: &Plane, window: Window) -> f32 {
        (window.w * source.w as f32) / (window.h * source.h as f32)
    }

    #[test]
    fn a_crop_is_found_at_its_position_and_scale_whatever_its_tones() {
        let source = picture(640, 480);
        for (x, y, w) in [(0.12, 0.08, 0.8), (0.03, 0.2, 0.75), (0.25, 0.1, 0.6)] {
            // A 4:3 frame inside a 4:3 picture: its height is the same fraction as its width.
            let window = Window { x, y, w, h: w };
            let reference = reference_of(&source, window, 300, |v| 0.55 * v + 0.2);
            let found = register(&source, &reference).unwrap();
            assert!(found.score > 0.98, "{x} {y} {w}: score {}", found.score);
            assert!(
                (found.window.x - window.x).abs() < 0.01
                    && (found.window.y - window.y).abs() < 0.01
                    && (found.window.w - window.w).abs() < 0.01,
                "{:?} vs {window:?}",
                found.window
            );
        }
    }

    #[test]
    fn a_flat_or_tiny_picture_is_not_registered() {
        let flat = Plane {
            w: 100,
            h: 80,
            data: vec![0.5; 8000],
        };
        let source = picture(200, 150);
        assert!(register(&source, &flat).is_none());
        assert!(register(&flat, &source).is_none());
        let tiny = Plane {
            w: 4,
            h: 4,
            data: vec![0.1; 16],
        };
        assert!(register(&tiny, &tiny).is_none());
    }

    #[test]
    fn the_box_filter_averages_and_never_enlarges() {
        let plane = Plane {
            w: 4,
            h: 2,
            data: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
        };
        let small = box_downscale(&plane, 2);
        assert_eq!((small.w, small.h), (2, 1));
        assert_eq!(small.data, vec![2.5, 4.5]);
        assert_eq!(box_downscale(&plane, 40).w, 4);
    }

    #[test]
    fn cropping_resamples_a_colour_image_and_refuses_bad_input() {
        let data: Vec<f32> = (0..8 * 6).flat_map(|i| [i as f32, 0.0, 1.0]).collect();
        let image = ImageF32::new(8, 6, 3, data).unwrap();
        let whole = Window {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        };
        let same = crop_resample(&image, whole, 8, 6).unwrap();
        assert!(same
            .data
            .iter()
            .zip(&image.data)
            .all(|(a, b)| (a - b).abs() < 1e-4));
        let gray = ImageF32::new(2, 2, 1, vec![0.0; 4]).unwrap();
        assert!(crop_resample(&gray, whole, 2, 2).is_err());
        assert!(crop_resample(&image, whole, 0, 2).is_err());
    }
}
