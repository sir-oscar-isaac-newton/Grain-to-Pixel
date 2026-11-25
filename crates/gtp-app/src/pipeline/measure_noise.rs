//! Crops for decision 0020 (wavelet noise reduction). Not a check: the test is ignored; it renders
//! the real scans of `samples/private` (git-ignored) to the picture before the noise stage, at full
//! size, with the automatic recipe, and writes a few 1024 x 1024 crops of each as raw files into
//! the directory named by the environment variable `GTP_NOISE_MEASURE_DIR` (outside the
//! repository). The figures themselves come from `gtp-edit`'s ignored `measure_` tests, which read
//! those files. Run with
//! `cargo test --release -p gtp-app measure_noise -- --ignored --nocapture --test-threads=1`.
//!
//! A file holds the width and the height as `u32`, then the pixels as `f32` triples (linear
//! Rec.2020), all little-endian.

use std::path::Path;
use std::sync::Arc;

use gtp_io::{read_tiff16, split_rgbi, to_f32_linear, Limits};

use super::{analyze, render_up_to_noise, PipelineInput, RenderTarget};

/// The scans measured: fine grain, middle and coarse.
const STOCKS: [(&str, &str); 3] = [
    ("ektar100", "Ektar100 C41 0001.tif"),
    ("vision3-250d", "Vision3 250D ECN2 0001.tif"),
    ("vision3-500t", "Vision3 500T 0001.tif"),
];
/// Side of a crop, in pixels.
const CROP: usize = 1024;
/// Step of the grid the flat-looking crop is searched on.
const GRID: usize = 256;

/// The square root of the mean absolute difference of neighbours of the cube root of the luminance
/// of a crop: small for a flat region whose texture is grain.
fn busyness(data: &[f32], width: usize, x0: usize, y0: usize, size: usize) -> f32 {
    let value = |x: usize, y: usize| {
        let p = &data[(y * width + x) * 3..(y * width + x) * 3 + 3];
        (0.2627 * p[0] + 0.678 * p[1] + 0.0593 * p[2])
            .max(0.0)
            .cbrt()
    };
    let mut sum = 0.0_f64;
    for y in y0..y0 + size - 1 {
        for x in x0..x0 + size - 1 {
            let v = value(x, y);
            sum += ((value(x + 1, y) - v).abs() + (value(x, y + 1) - v).abs()) as f64;
        }
    }
    (sum / (size * size) as f64) as f32
}

fn write_crop(
    dir: &Path,
    name: &str,
    data: &[f32],
    width: usize,
    (x0, y0, size): (usize, usize, usize),
) {
    let mut bytes = Vec::with_capacity(8 + size * size * 12);
    bytes.extend_from_slice(&(size as u32).to_le_bytes());
    bytes.extend_from_slice(&(size as u32).to_le_bytes());
    for y in y0..y0 + size {
        for v in &data[(y * width + x0) * 3..(y * width + x0 + size) * 3] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    std::fs::write(dir.join(name), bytes).unwrap();
}

#[test]
#[ignore = "measurement, not a check; needs samples/private and GTP_NOISE_MEASURE_DIR"]
fn measure_noise_crops_of_real_scans() {
    let Some(out) = std::env::var_os("GTP_NOISE_MEASURE_DIR") else {
        eprintln!("GTP_NOISE_MEASURE_DIR is not set: nothing written");
        return;
    };
    let out = Path::new(&out);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/private");
    if !dir.is_dir() {
        eprintln!("samples/private is missing: nothing written");
        return;
    }
    for (stock, file) in STOCKS {
        let path = dir.join(file);
        if !path.is_file() {
            eprintln!("{file}: not found");
            continue;
        }
        let tiff = read_tiff16(&path, &Limits::default()).unwrap();
        let rgb = if tiff.channels == 4 {
            split_rgbi(&tiff).unwrap().0
        } else {
            tiff
        };
        let input = PipelineInput {
            rgb: Arc::new(to_f32_linear(&rgb).unwrap()),
            ir: None,
            film_base: None,
            full_long_edge: None,
        };
        let auto = analyze(&input).unwrap().auto_recipe;
        // The automatic recipe has both noise amounts at 0, so this is the picture the stage gets.
        let image = render_up_to_noise(&input, &auto, RenderTarget::Full, &mut |_| {}).unwrap();
        let (width, height) = (image.width as usize, image.height as usize);
        let size = CROP.min(width).min(height);
        eprintln!("{stock}: {file}, rendered {width} x {height}, crops of {size}");
        let mut flattest = (f32::MAX, 0, 0);
        for y in (0..=height - size).step_by(GRID) {
            for x in (0..=width - size).step_by(GRID) {
                let score = busyness(&image.data, width, x, y, size);
                if score < flattest.0 {
                    flattest = (score, x, y);
                }
            }
        }
        eprintln!(
            "  flattest crop at ({}, {}), busyness {}",
            flattest.1, flattest.2, flattest.0
        );
        let spots = [
            ("flat", flattest.1, flattest.2),
            ("a", (width - size) / 4, (height - size) * 3 / 10),
            ("b", (width - size) / 2, (height - size) / 2),
            ("c", (width - size) * 3 / 4, (height - size) * 7 / 10),
        ];
        for (tag, x0, y0) in spots {
            write_crop(
                out,
                &format!("{stock}-{tag}.crop"),
                &image.data,
                width,
                (x0, y0, size),
            );
        }
    }
}
