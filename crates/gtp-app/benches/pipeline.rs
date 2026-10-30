//! Pipeline benchmark: a 1600 px preview render, the interactive budget of 100 ms
//! (docs/specification.md section 11). Run with `cargo bench -p gtp-app`.

use std::hint::black_box;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use gtp_app::{render, PipelineInput, RenderTarget, PREVIEW_EDGE};
use gtp_core::testing::synthetic::{self, Params};
use gtp_core::Recipe;

fn preview(c: &mut Criterion) {
    let params = Params {
        width: 2048,
        height: 1365,
        rebate: [60; 4],
        ..Params::default()
    };
    let Ok(scan) = synthetic::negative(1, &params) else {
        return;
    };
    let mut recipe = Recipe::default();
    recipe.base.rgb = scan.truth.base;
    recipe.invert.black = scan.truth.black;
    recipe.invert.white = scan.truth.white;
    recipe.tone.exposure_ev = 0.3;
    recipe.tone.contrast = 20.0;
    recipe.tone.shadows = 20.0;
    recipe.color.vibrance = 15.0;
    recipe.geometry.angle_deg = 1.0;
    let input = PipelineInput {
        rgb: Arc::new(scan.image),
        ir: None,
        film_base: None,
    };
    let target = RenderTarget::Preview {
        max_edge: PREVIEW_EDGE,
    };
    c.bench_function("render 1600 px preview from a 2048 px proxy", |b| {
        b.iter(|| black_box(render(black_box(&input), black_box(&recipe), target)));
    });
}

criterion_group!(benches, preview);
criterion_main!(benches);
