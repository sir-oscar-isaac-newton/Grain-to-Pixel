//! Proxies: smaller copies of a frame for interactive work (docs/specification.md section 11).
//!
//! The Develop view keeps a [`PROXY_EDGE`] proxy of the open frame and renders previews at
//! [`PREVIEW_EDGE`]. Downscaling uses an area (box) filter: it averages, so it never overshoots at
//! the sharp rebate edge, which keeps film-base measurement and frame detection reliable on proxies.

use fast_image_resize::images::TypedImage;
use fast_image_resize::pixels::F32x3;
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use gtp_core::{AppError, ErrorCode, ImageF32};
use rayon::prelude::*;

/// Long edge of the proxy kept for an open frame.
pub const PROXY_EDGE: u32 = 2048;
/// Long edge of interactive previews (while a slider is dragged).
pub const PREVIEW_EDGE: u32 = 1600;

/// A copy of `image` whose long edge is at most `max_edge`, keeping the aspect ratio. An image
/// that is already small enough is returned unchanged (as a copy). Needs 3 channels.
pub fn build_proxy(image: &ImageF32, max_edge: u32) -> Result<ImageF32, AppError> {
    if image.channels != 3 {
        return Err(
            AppError::new(ErrorCode::InvalidInput, "A proxy needs a 3-channel image.")
                .with_details(format!("{} channels", image.channels)),
        );
    }
    if max_edge == 0 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "A proxy needs a size above 0.",
        ));
    }
    let (w, h) = (image.width, image.height);
    let long = w.max(h);
    if long <= max_edge {
        return Ok(image.clone());
    }
    let scale = f64::from(max_edge) / f64::from(long);
    let shrink = |side: u32| ((f64::from(side) * scale).round() as u32).clamp(1, max_edge);
    let (new_w, new_h) = if w >= h {
        (max_edge, shrink(h))
    } else {
        (shrink(w), max_edge)
    };

    // The resize works on its own pixel type, so the pixels are copied in and out; those copies
    // and the resize itself (the library's `rayon` feature) use all cores. Every pixel is
    // computed from the same inputs as before, so the result is bit for bit the same.
    let (pixels, _) = image.data.as_chunks::<3>();
    let source_pixels: Vec<F32x3> = pixels.par_iter().map(|p| F32x3::new(*p)).collect();
    let resize_error =
        |err: &dyn std::fmt::Display| AppError::internal(format!("proxy resize: {err}"));
    let source = TypedImage::from_pixels(w, h, source_pixels).map_err(|e| resize_error(&e))?;
    let mut target = TypedImage::<F32x3>::new(new_w, new_h);
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Box));
    Resizer::new()
        .resize_typed(&source, &mut target, &options)
        .map_err(|e| resize_error(&e))?;
    let mut data = vec![0.0f32; new_w as usize * new_h as usize * 3];
    data.par_chunks_mut(3)
        .zip(target.pixels().par_iter())
        .for_each(|(out, pixel)| out.copy_from_slice(&pixel.0));
    ImageF32::new(new_w, new_h, 3, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: u32, h: u32, value: f32) -> ImageF32 {
        ImageF32::new(w, h, 3, vec![value; (w * h * 3) as usize]).unwrap()
    }

    /// `build_proxy` as it was before it was sped up (P-17): the same resize, with the pixels
    /// copied in and out one at a time on one thread. The fast version must give the same bits.
    fn reference_proxy(image: &ImageF32, max_edge: u32) -> ImageF32 {
        let (w, h) = (image.width, image.height);
        let long = w.max(h);
        if long <= max_edge {
            return image.clone();
        }
        let scale = f64::from(max_edge) / f64::from(long);
        let shrink = |side: u32| ((f64::from(side) * scale).round() as u32).clamp(1, max_edge);
        let (new_w, new_h) = if w >= h {
            (max_edge, shrink(h))
        } else {
            (shrink(w), max_edge)
        };
        let (pixels, _) = image.data.as_chunks::<3>();
        let source_pixels: Vec<F32x3> = pixels.iter().map(|p| F32x3::new(*p)).collect();
        let source = TypedImage::from_pixels(w, h, source_pixels).unwrap();
        let mut target = TypedImage::<F32x3>::new(new_w, new_h);
        let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Box));
        Resizer::new()
            .resize_typed(&source, &mut target, &options)
            .unwrap();
        let data = target.pixels().iter().flat_map(|p| p.0).collect();
        ImageF32::new(new_w, new_h, 3, data).unwrap()
    }

    /// Deterministic noise in 0 to 1.5 (some above 1, as a negative can be), so every pixel and
    /// channel differs and a mix-up of channels or rows would show.
    fn noisy(w: u32, h: u32, seed: u32) -> ImageF32 {
        let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
        let data = (0..w * h * 3)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state % 15_000) as f32 / 10_000.0
            })
            .collect();
        ImageF32::new(w, h, 3, data).unwrap()
    }

    #[test]
    fn the_fast_proxy_has_exactly_the_bits_of_the_original_one() {
        for (w, h, edge) in [
            (2048, 1365, PREVIEW_EDGE),
            (2048, 2048, PREVIEW_EDGE),
            (1365, 2048, PREVIEW_EDGE),
            (3001, 2003, PROXY_EDGE),
            (2047, 1301, 1000),
            (4097, 301, 256),
            (301, 4097, 600),
            (5, 3, 2),
        ] {
            let image = noisy(w, h, w ^ h);
            assert_eq!(
                build_proxy(&image, edge).unwrap(),
                reference_proxy(&image, edge),
                "{w}x{h} to {edge}"
            );
        }
    }

    #[test]
    fn proxies_have_the_requested_long_edge_and_keep_the_aspect_ratio() {
        for ((w, h), edge, want) in [
            ((6000, 4000), PROXY_EDGE, (2048, 1365)),
            ((6000, 4000), PREVIEW_EDGE, (1600, 1067)),
            ((4000, 6000), PROXY_EDGE, (1365, 2048)),
            ((5959, 3946), PREVIEW_EDGE, (1600, 1060)),
        ] {
            let proxy = build_proxy(&flat(w, h, 0.4), edge).unwrap();
            assert_eq!((proxy.width, proxy.height), want, "{w}x{h} -> {edge}");
        }
    }

    #[test]
    fn a_small_image_is_returned_as_is() {
        let small = flat(800, 600, 0.4);
        assert_eq!(build_proxy(&small, PREVIEW_EDGE).unwrap(), small);
    }

    #[test]
    fn the_area_filter_keeps_flat_values_and_never_overshoots() {
        let proxy = build_proxy(&flat(3000, 2000, 0.37), 1000).unwrap();
        assert!(proxy.data.iter().all(|v| (v - 0.37).abs() < 1e-5));

        // A hard edge (like rebate against picture) stays inside its two levels.
        let mut edge = flat(3000, 2000, 0.05);
        for y in 0..2000usize {
            for x in 0..1000usize {
                edge.data[(y * 3000 + x) * 3..(y * 3000 + x) * 3 + 3].fill(1.0);
            }
        }
        let proxy = build_proxy(&edge, 1000).unwrap();
        assert!(proxy
            .data
            .iter()
            .all(|v| (0.05 - 1e-5..=1.0 + 1e-5).contains(v)));
    }

    #[test]
    fn bad_input_is_rejected() {
        let gray = ImageF32::zeros(10, 10, 1).unwrap();
        assert_eq!(
            build_proxy(&gray, 5).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        assert_eq!(
            build_proxy(&flat(10, 10, 0.1), 0).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
}
