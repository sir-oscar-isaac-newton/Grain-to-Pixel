//! Figures for decision 0021 (a flat field that leaves the film carrier out). Not a check: the test
//! is ignored and reads the real bare-light capture of `samples/private` (git-ignored), skipping with
//! a message when it is not there. Run with
//! `cargo test --release -p gtp-app flat_field::measure -- --ignored --nocapture --test-threads=1`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use gtp_core::{FlatField, ImageF32};
use gtp_ingest::merge_triplet;
use gtp_io::{Limits, RawlerDecoder};

use super::reference::build_flat_field_reference;
use super::*;

const CAPTURE: &str =
    "samples/private/Mamiya 645 Kodak Gold 200 120 Trichrome RGB/Barelight RGB Capture";

fn sources() -> Vec<String> {
    vec!["r.NEF".into(), "g.NEF".into(), "b.NEF".into()]
}

fn capture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(CAPTURE)
}

/// The three exposures of the capture in red, green, blue order, told apart by colour.
fn exposures(dir: &Path) -> Option<[PathBuf; 3]> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("nef"))
        })
        .collect();
    files.sort();
    let candidates = triplet_group(&files).ok()?;
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    let [r, g, b] = candidate.files.as_slice() else {
        return None;
    };
    Some([
        r.path.clone().into(),
        g.path.clone().into(),
        b.path.clone().into(),
    ])
}

/// The cells within two cells of a covered cell (the two cells inside the opening's edge); when
/// nothing is covered, the two outer rings of the grid.
fn band_cells(flat: &FlatField, covered: &[bool]) -> Vec<(usize, usize)> {
    let (gw, gh) = (flat.grid_w as usize, flat.grid_h as usize);
    let any_covered = covered.iter().any(|c| *c);
    let mut band = Vec::new();
    for gy in 0..gh {
        for gx in 0..gw {
            if covered[gy * gw + gx] {
                continue;
            }
            let near = if any_covered {
                (gy.saturating_sub(2)..=(gy + 2).min(gh - 1)).any(|ny| {
                    (gx.saturating_sub(2)..=(gx + 2).min(gw - 1)).any(|nx| covered[ny * gw + nx])
                })
            } else {
                gx < 2 || gy < 2 || gx + 2 >= gw || gy + 2 >= gh
            };
            if near {
                band.push((gx, gy));
            }
        }
    }
    band
}

/// Mean, least and greatest of the values.
fn spread(values: &[f64]) -> (f64, f64, f64) {
    let mean = values.iter().sum::<f64>() / values.len().max(1) as f64;
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (mean, min, max)
}

/// The deviation from the centre of the capture divided by `flat`, over the samples of the band cells that
/// the new method kept (so not the carrier, nor its margin): mean and greatest, per channel.
fn evenness(
    image: &ImageF32,
    flat: &FlatField,
    stats: &BuildStats,
    band: &[(usize, usize)],
) -> [(f64, f64, f64, f64); 3] {
    let mut corrected = image.clone();
    apply_flat_field(&mut corrected, flat).unwrap();
    let (w, h) = (image.width as usize, image.height as usize);
    // The capture is not 1 at its centre, so the corrected picture is compared with its own mean
    // over the middle 200 x 200 pixels.
    let mut centre = [0.0f64; 3];
    for y in h / 2 - 100..h / 2 + 100 {
        for x in w / 2 - 100..w / 2 + 100 {
            for (ch, c) in centre.iter_mut().enumerate() {
                *c += f64::from(corrected.data[(y * w + x) * 3 + ch]) / 40_000.0;
            }
        }
    }
    let n_x = stats.sample_x.len();
    let mut devs: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (gx, gy) in band {
        for sy in stats.cell_rows[*gy].clone() {
            for sx in stats.cell_columns[*gx].clone() {
                if stats.excluded[sy * n_x + sx] != 0 {
                    continue;
                }
                let (x, y) = (stats.sample_x[sx], stats.sample_y[sy]);
                for (ch, d) in devs.iter_mut().enumerate() {
                    d.push(
                        (f64::from(corrected.data[(y * w + x) * 3 + ch]) / centre[ch] - 1.0).abs(),
                    );
                }
            }
        }
    }
    devs.map(|mut d| {
        d.sort_by(f64::total_cmp);
        let n = d.len().max(1);
        let mean = d.iter().sum::<f64>() / n as f64;
        let over = d.iter().filter(|v| **v > 0.03).count() as f64 / n as f64;
        (
            mean,
            d.get(n * 99 / 100).copied().unwrap_or(0.0),
            over,
            d.last().copied().unwrap_or(0.0),
        )
    })
}

fn band_report(old: &FlatField, new: &FlatField, band: &[(usize, usize)]) -> String {
    let gw = new.grid_w as usize;
    let mut out = String::new();
    let mut worst = 0.0f64;
    for ch in 0..3 {
        let ratios: Vec<f64> = band
            .iter()
            .map(|(gx, gy)| {
                let i = (gy * gw + gx) * 3 + ch;
                f64::from(old.light[i]) / f64::from(new.light[i])
            })
            .collect();
        let (mean, min, max) = spread(&ratios);
        worst = worst.max((min - 1.0).abs()).max((max - 1.0).abs());
        out += &format!("  channel {ch}: old/new mean {mean:.4} min {min:.4} max {max:.4}\n");
    }
    out + &format!("  max relative difference old against new: {worst:.4}\n")
}

/// The light and what each flat field leaves of it by distance from the carrier, in samples, over every
/// kept or left-out sample (no cell and no margin left out, unlike [`evenness`]).
fn print_profile(image: &ImageF32, flats: [&FlatField; 2], covered: &BuildStats) {
    let (w, h) = (image.width as usize, image.height as usize);
    let n_x = covered.sample_x.len();
    let corrected = flats.map(|flat| {
        let mut picture = image.clone();
        apply_flat_field(&mut picture, flat).unwrap();
        picture
    });
    let centre = |picture: &ImageF32| {
        let mut sum = 0.0f64;
        for y in h / 2 - 100..h / 2 + 100 {
            for x in w / 2 - 100..w / 2 + 100 {
                sum += f64::from(picture.data[(y * w + x) * 3 + 1]);
            }
        }
        sum / 40_000.0
    };
    let pictures = [image, &corrected[0], &corrected[1]];
    let centres = pictures.map(centre);
    println!("profile, green (samples from the carrier: count; raw mean min; old mean max; new mean min max, all as a share of the centre):");
    let mut before = covered.excluded.clone();
    for d in 1..=40usize {
        let now = dilate(&covered.excluded, n_x, d);
        let mut v: [Vec<f64>; 3] = Default::default();
        for (i, (a, b)) in now.iter().zip(&before).enumerate() {
            if *a == 0 || *b != 0 {
                continue;
            }
            let at = (covered.sample_y[i / n_x] * w + covered.sample_x[i % n_x]) * 3 + 1;
            for (k, list) in v.iter_mut().enumerate() {
                list.push(f64::from(pictures[k].data[at]) / centres[k]);
            }
        }
        before = now;
        if d > 12 && d % 4 != 0 {
            continue;
        }
        let [raw, old, new] = v.map(|list| (spread(&list), list.len()));
        println!(
            "  {d}: {}; {:.3} {:.3}; {:.3} {:.3}; {:.3} {:.3} {:.3}",
            raw.1, raw.0 .0, raw.0 .1, old.0 .0, old.0 .2, new.0 .0, new.0 .1, new.0 .2
        );
    }
}

fn median_ms(mut times: Vec<f64>) -> f64 {
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

#[test]
#[ignore = "reads the real capture of samples/private and prints figures"]
fn measure_the_flat_field_of_the_real_bare_light() {
    let dir = capture_dir();
    let Some([r, g, b]) = exposures(&dir) else {
        println!("skipped: no bare-light triplet in {}", dir.display());
        return;
    };
    let merged = merge_triplet(&RawlerDecoder, &r, &g, &b, &Limits::default())
        .unwrap()
        .image;
    println!(
        "capture: {} {} {}, {}x{} pixels",
        r.file_name().unwrap().to_string_lossy(),
        g.file_name().unwrap().to_string_lossy(),
        b.file_name().unwrap().to_string_lossy(),
        merged.width,
        merged.height
    );

    let old = build_flat_field_reference(&merged, sources()).unwrap();
    let (new, stats) = build_flat_field_with(&merged, sources(), CarrierParams::default()).unwrap();
    let gw = new.grid_w as usize;
    println!("grid: {} x {} cells", new.grid_w, new.grid_h);
    println!(
        "covered cells: {} of {}",
        stats.covered_cells,
        stats.cell_covered.len()
    );
    println!(
        "covered samples: {} of {} ({:.4} %), left out with the margin: {} ({:.4} %)",
        stats.covered_samples,
        stats.samples,
        100.0 * stats.covered_samples as f64 / stats.samples as f64,
        stats.excluded_samples,
        100.0 * stats.excluded_samples as f64 / stats.samples as f64
    );
    let open: Vec<usize> = (0..stats.cell_covered.len())
        .filter(|i| !stats.cell_covered[*i])
        .collect();
    let least = open
        .iter()
        .flat_map(|i| (0..3).map(move |ch| (i, ch)))
        .map(|(i, ch)| new.light[i * 3 + ch])
        .fold(f32::INFINITY, f32::min);
    let greatest = open
        .iter()
        .flat_map(|i| (0..3).map(move |ch| (i, ch)))
        .map(|(i, ch)| new.light[i * 3 + ch])
        .fold(f32::NEG_INFINITY, f32::max);
    let darkest_brightest_channel = open
        .iter()
        .map(|i| {
            new.light[i * 3..i * 3 + 3]
                .iter()
                .copied()
                .fold(0.0f32, f32::max)
        })
        .fold(f32::INFINITY, f32::min);
    println!(
        "open cells: light min {least:.4} max {greatest:.4}; darkest brightest-channel light {darkest_brightest_channel:.4} (carrier level {CARRIER_LEVEL})"
    );
    let band = band_cells(&new, &stats.cell_covered);
    println!(
        "band: {} cells {} the opening edge",
        band.len(),
        if stats.covered_cells > 0 {
            "within 2 cells inside"
        } else {
            "(no carrier found: the two outer rings of the grid)"
        }
    );
    print!("{}", band_report(&old, &new, &band));
    for (name, flat) in [("old", &old), ("new", &new)] {
        let e = evenness(&merged, flat, &stats, &band);
        println!(
            "evenness of the bare light divided by its {name} flat field, in the band, kept samples (mean, 99th percentile, share beyond 3 %, max deviation from the centre): R {:.4} {:.4} {:.4} {:.4}, G {:.4} {:.4} {:.4} {:.4}, B {:.4} {:.4} {:.4} {:.4}",
            e[0].0, e[0].1, e[0].2, e[0].3, e[1].0, e[1].1, e[1].2, e[1].3, e[2].0, e[2].1, e[2].2, e[2].3
        );
    }
    let (_, carrier) = build_flat_field_with(
        &merged,
        sources(),
        CarrierParams {
            margin: 0,
            ..CarrierParams::default()
        },
    )
    .unwrap();
    print_profile(&merged, [&old, &new], &carrier);
    // Wider margins, and where the samples well away from the carrier that stay off are.
    for m in [CARRIER_MARGIN_SAMPLES, 8, 16] {
        let (f, st) = build_flat_field_with(
            &merged,
            sources(),
            CarrierParams {
                level: 0.25,
                margin: m,
            },
        )
        .unwrap();
        println!("MARGIN {m} covered cells {}", st.covered_cells);
        print_profile(&merged, [&old, &f], &carrier);
        let mut c = merged.clone();
        apply_flat_field(&mut c, &f).unwrap();
        let (w, n_x) = (c.width as usize, carrier.sample_x.len());
        let far = dilate(&carrier.excluded, n_x, 12);
        let gc = f64::from(c.data[(c.height as usize / 2 * w + w / 2) * 3 + 1]);
        let mut hist = std::collections::BTreeMap::new();
        for (i, e) in far.iter().enumerate() {
            if *e != 0 {
                continue;
            }
            let (x, y) = (carrier.sample_x[i % n_x], carrier.sample_y[i / n_x]);
            let v = f64::from(c.data[(y * w + x) * 3 + 1]) / gc;
            if (v - 1.0).abs() > 0.05 {
                let e = hist
                    .entry((x / 250 * 250, y / 250 * 250))
                    .or_insert((0usize, 9.0f64, 0.0f64));
                e.0 += 1;
                e.1 = e.1.min(v);
                e.2 = e.2.max(v);
            }
        }
        println!("  beyond 5% and over 12 samples away, by 250 px tile: {hist:?}");
    }

    // Time of the build, release: median of 5.
    let time = |f: &dyn Fn()| {
        median_ms(
            (0..5)
                .map(|_| {
                    let t = Instant::now();
                    f();
                    t.elapsed().as_secs_f64() * 1000.0
                })
                .collect(),
        )
    };
    let old_ms = time(&|| {
        std::hint::black_box(build_flat_field_reference(&merged, sources()).unwrap());
    });
    let new_ms = time(&|| {
        std::hint::black_box(build_flat_field(&merged, sources()).unwrap());
    });
    println!("build time, median of 5: old {old_ms:.1} ms, new {new_ms:.1} ms");

    // Sensitivity.
    println!("sensitivity (level, margin: covered cells, covered share, left out share, band max difference, darkest open light):");
    for level in [0.15, 0.25, 0.35] {
        for margin in [2usize, 4, 8] {
            let Ok((flat, s)) =
                build_flat_field_with(&merged, sources(), CarrierParams { level, margin })
            else {
                println!("  level {level}, margin {margin}: refused");
                continue;
            };
            let band = band_cells(&flat, &s.cell_covered);
            let mut worst = 0.0f64;
            for (gx, gy) in &band {
                for ch in 0..3 {
                    let i = (gy * gw + gx) * 3 + ch;
                    worst =
                        worst.max((f64::from(old.light[i]) / f64::from(flat.light[i]) - 1.0).abs());
                }
            }
            let darkest = (0..s.cell_covered.len())
                .filter(|i| !s.cell_covered[*i])
                .flat_map(|i| (0..3).map(move |ch| (i, ch)))
                .map(|(i, ch)| flat.light[i * 3 + ch])
                .fold(f32::INFINITY, f32::min);
            println!(
                "  level {level}, margin {margin}: {} cells, {:.4} %, {:.4} %, {worst:.4}, {darkest:.4}",
                s.covered_cells,
                100.0 * s.covered_samples as f64 / s.samples as f64,
                100.0 * s.excluded_samples as f64 / s.samples as f64
            );
        }
    }
}
