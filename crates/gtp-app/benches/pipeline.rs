//! Pipeline benchmarks against the budgets of docs/specification.md section 11: decoding a 21 MP
//! TIFF, building the proxy, a 1600 px preview render (100 ms), the automatic analysis on a proxy
//! (500 ms) and a full-resolution render (the core of the 10 s export). Run with
//! `cargo bench -p gtp-app`; the results are in docs/research/benchmark.md.

use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};
use gtp_app::{
    analyze, build_proxy, render, PipelineInput, RenderTarget, PREVIEW_EDGE, PROXY_EDGE,
};
use gtp_core::testing::synthetic::{self, Params, Synthetic};
use gtp_core::testing::TempDir;
use gtp_core::{ImageF32, Recipe};
use gtp_io::{from_f32_linear, read_tiff16, to_f32_linear, write_tiff16, Compression, Limits};

/// A 21 MP frame (a D600's 6016 x 4016) and the recipe the benchmarks render it with.
const FULL_SIZE: (u32, u32) = (6016, 4016);

fn negative(width: u32, height: u32, rebate: u32) -> Option<Synthetic> {
    let params = Params {
        width,
        height,
        rebate: [rebate; 4],
        ..Params::default()
    };
    synthetic::negative(1, &params).ok()
}

/// The image at twice the size by repeating pixels: the synthetic negatives stop at 4096 px a side.
fn doubled(image: &ImageF32) -> Option<ImageF32> {
    let (w, h) = (image.width as usize, image.height as usize);
    let mut data = Vec::with_capacity(w * h * 4 * 3);
    for y in 0..h {
        let row = &image.data[y * w * 3..(y + 1) * w * 3];
        let wide: Vec<f32> = row
            .chunks(3)
            .flat_map(|p| [p, p])
            .flatten()
            .copied()
            .collect();
        data.extend_from_slice(&wide);
        data.extend_from_slice(&wide);
    }
    ImageF32::new(image.width * 2, image.height * 2, 3, data).ok()
}

fn recipe_for(scan: &Synthetic) -> Recipe {
    let mut recipe = Recipe::default();
    recipe.base.rgb = scan.truth.base;
    recipe.invert.black = scan.truth.black;
    recipe.invert.white = scan.truth.white;
    recipe.tone.exposure_ev = 0.3;
    recipe.tone.contrast = 20.0;
    recipe.tone.shadows = 20.0;
    recipe.color.vibrance = 15.0;
    recipe.geometry.angle_deg = 1.0;
    recipe
}

fn preview(c: &mut Criterion) {
    let Some(scan) = negative(2048, 1365, 60) else {
        return;
    };
    let recipe = recipe_for(&scan);
    let input = PipelineInput {
        rgb: Arc::new(scan.image),
        ir: None,
        film_base: None,
        full_long_edge: None,
    };
    let target = RenderTarget::Preview {
        max_edge: PREVIEW_EDGE,
    };
    c.bench_function("render 1600 px preview from a 2048 px proxy", |b| {
        b.iter(|| black_box(render(black_box(&input), black_box(&recipe), target)));
    });
}

/// Analysis on the cached proxy, as Develop does it: frame detection, film base and levels.
fn analysis(c: &mut Criterion) {
    let Some(scan) = negative(PROXY_EDGE, PROXY_EDGE * 2 / 3, 60) else {
        return;
    };
    let input = PipelineInput {
        rgb: Arc::new(scan.image),
        ir: None,
        film_base: None,
        full_long_edge: None,
    };
    c.bench_function("analyze a proxy", |b| {
        b.iter(|| black_box(analyze(black_box(&input))));
    });
}

/// What opening a 21 MP frame costs (decode the TIFF, build the proxy) and rendering it in full.
fn full_resolution(c: &mut Criterion) {
    let Some(mut scan) = negative(FULL_SIZE.0 / 2, FULL_SIZE.1 / 2, 100) else {
        return;
    };
    let Some(image) = doubled(&scan.image) else {
        return;
    };
    scan.image = image;
    let recipe = recipe_for(&scan);

    let dir = TempDir::new("bench");
    let path = dir.join("full.tif");
    let written = from_f32_linear(&scan.image);
    if write_tiff16(&path, &written, Compression::None).is_ok() {
        let mut group = c.benchmark_group("open a 21 MP frame");
        group
            .sample_size(10)
            .measurement_time(Duration::from_secs(20));
        group.bench_function("decode the TIFF to f32", |b| {
            b.iter(|| {
                let tiff = read_tiff16(black_box(&path), &Limits::default());
                black_box(tiff.and_then(|t| to_f32_linear(&t)))
            });
        });
        group.bench_function("build the proxy", |b| {
            b.iter(|| black_box(build_proxy(black_box(&scan.image), PROXY_EDGE)));
        });
        group.finish();
    }
    let _ = std::fs::remove_file(&path);

    let input = PipelineInput {
        rgb: Arc::new(scan.image),
        ir: None,
        film_base: None,
        full_long_edge: None,
    };
    let mut group = c.benchmark_group("full resolution");
    group
        .sample_size(10)
        .measurement_time(Duration::from_secs(30));
    group.bench_function("render a 21 MP frame", |b| {
        b.iter(|| {
            black_box(render(
                black_box(&input),
                black_box(&recipe),
                RenderTarget::Full,
            ))
        });
    });
    group.bench_function("analyze a 21 MP frame (proxy included)", |b| {
        b.iter(|| black_box(analyze(black_box(&input))));
    });
    group.finish();
}

criterion_group!(benches, preview, analysis, full_resolution);
criterion_main!(benches);
