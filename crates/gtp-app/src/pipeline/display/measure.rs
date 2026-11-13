//! Timing for decision 0015 (one colour conversion for screen and file): the screen's conversion
//! (`to_rgba8_srgb`) against the export's (`gtp_io::to_output` to sRGB, then 16 to 8 bits, to RGBA8
//! as a screen would need it). Not a check; run it with
//! `cargo test --release -p gtp-app time_screen_and_export -- --ignored --nocapture`.
//! The comparison of the two on colours is the proof in `against_export.rs`.

use std::time::{Duration, Instant};

use gtp_core::recipe::OutputSpace;
use gtp_io::to_output;

use super::*;

fn median(mut runs: Vec<Duration>) -> Duration {
    runs.sort();
    runs[runs.len() / 2]
}

fn time(f: &dyn Fn()) -> Duration {
    f();
    median(
        (0..7)
            .map(|_| {
                let started = Instant::now();
                f();
                started.elapsed()
            })
            .collect(),
    )
}

/// The export's conversion to RGBA8: `to_output`, then 16 to 8 bits.
fn export_rgba8(image: &ImageF32) -> Vec<u8> {
    let output = to_output(image, OutputSpace::Srgb).unwrap();
    let (pixels, _) = output.data.as_chunks::<3>();
    let mut out = vec![0u8; pixels.len() * 4];
    out.par_chunks_mut(PIXELS_PER_TASK * 4)
        .zip(pixels.par_chunks(PIXELS_PER_TASK))
        .for_each(|(rgba_run, rgb_run)| {
            let (rgba_run, _) = rgba_run.as_chunks_mut::<4>();
            for (rgba, rgb) in rgba_run.iter_mut().zip(rgb_run) {
                for (slot, v) in rgba.iter_mut().zip(rgb) {
                    *slot = ((u32::from(*v) + 128) / 257) as u8;
                }
                rgba[3] = 255;
            }
        });
    out
}

/// Median of seven runs of each path, on pools of 6 and 12 threads.
#[test]
#[ignore = "timing run, not a check"]
fn time_screen_and_export() {
    for threads in [6usize, 12] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        for (label, w, h) in [
            ("1600x1067", 1600u32, 1067u32),
            ("2048x1365", 2048, 1365),
            ("6000x4000", 6000, 4000),
        ] {
            let mut state = 0x9E37_79B9u32;
            let data = (0..w as usize * h as usize * 3)
                .map(|i| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    (i / 3 % w as usize) as f32 / w as f32 * 0.9 + (state % 1000) as f32 / 10_000.0
                })
                .collect();
            let image = ImageF32::new(w, h, 3, data).unwrap();
            let screen = time(&|| {
                pool.install(|| to_rgba8_srgb(&image).unwrap());
            });
            let export = time(&|| {
                pool.install(|| export_rgba8(&image));
            });
            eprintln!(
                "{threads} threads {label}: screen (to_rgba8_srgb) {screen:?}; export path (to_output, uncached transform, RGBA8) {export:?}"
            );
        }
    }
}
