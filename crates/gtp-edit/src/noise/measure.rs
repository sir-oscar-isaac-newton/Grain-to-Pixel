//! Figures for decision 0020 (wavelet noise reduction). Not checks: each test is ignored and prints
//! its table; run them with
//! `cargo test --release -p gtp-edit measure_ -- --ignored --nocapture --test-threads=1`.
//!
//! **The measure.** The lightness plane `L` (Oklab) is analysed with the a trous transform of the
//! reduction itself (B3 spline kernel dilated by `2^j`, scales 0 to 4, no thresholding). The energy
//! of a scale is the mean of its squared detail over the picture without a border of 64 pixels.
//! After a reduction, the fraction of the input's energy that is left at a scale is the "kept"
//! figure of that scale. The finest scale (0) is the grain texture; scales 2 to 4 together are the
//! coarse blotches. A method keeps grain when, at the lowest amount where the coarse energy has
//! dropped to a half or less, it keeps a clearly larger part of the finest scale.
//!
//! The crops of real scans come from `gtp-app`'s `measure_noise_crops_of_real_scans`, from the
//! directory named by `GTP_NOISE_MEASURE_DIR`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::reference::{coarse, energies, grain};
use super::tests::{gauss_for_measure, stream_for_measure};
use super::*;

/// Amounts of the tables.
const AMOUNTS: [f32; 4] = [25.0, 50.0, 75.0, 100.0];
/// The guided filter of P-27 for luminance (what this issue measures against): its radius in
/// pixels and the noise level at an amount of 100, in Oklab lightness.
const GUIDED_RADIUS_PX: usize = 3;
const GUIDED_MAX_SIGMA: f32 = 0.04;

/// The luminance of P-27: the self-guided filter of `l`.
fn guided_luminance(l: &[f32], width: usize, height: usize, amount: f32) -> Vec<f32> {
    let sigma = amount / NOISE_AMOUNT_MAX * GUIDED_MAX_SIGMA;
    let mut out = guided_filter_in(
        &mut Workspace::default(),
        l,
        &[l],
        width,
        height,
        GUIDED_RADIUS_PX,
        sigma * sigma,
    );
    out.swap_remove(0)
}

fn wavelet_luminance(
    l: &[f32],
    width: usize,
    height: usize,
    amount: f32,
    strength: f32,
) -> Vec<f32> {
    let mut out = l.to_vec();
    wavelet_luminance_in(
        &mut Workspace::default(),
        &mut out,
        (width, height),
        amount,
        1.0,
        strength,
    );
    out
}

/// (fraction of the finest scale kept, fraction of the coarse scales kept) of `after` against the
/// energies of the input, summed over crops.
fn kept(before: &[[f64; 5]], after: &[[f64; 5]]) -> (f64, f64) {
    let fine_before: f64 = before.iter().map(|e| e[0]).sum();
    let fine_after: f64 = after.iter().map(|e| e[0]).sum();
    let coarse_before: f64 = before.iter().map(coarse).sum();
    let coarse_after: f64 = after.iter().map(coarse).sum();
    (fine_after / fine_before, coarse_after / coarse_before)
}

/// The finest-scale figure at the point where the coarse figure crosses a half, interpolated
/// linearly between the amounts of `curve` (amount, fine, coarse), which starts at (0, 1, 1).
fn fine_at_half_coarse(curve: &[(f32, f64, f64)]) -> Option<(f32, f64)> {
    curve.windows(2).find_map(|pair| {
        let ((a0, f0, c0), (a1, f1, c1)) = (pair[0], pair[1]);
        (c0 > 0.5 && c1 <= 0.5).then(|| {
            let t = (0.5 - c0) / (c1 - c0);
            (a0 + (a1 - a0) * t as f32, f0 + (f1 - f0) * t)
        })
    })
}

type Crop = (Vec<f32>, usize, usize);

fn load_crop(path: &Path) -> Option<Crop> {
    let bytes = std::fs::read(path).ok()?;
    let width = u32::from_le_bytes(bytes[0..4].try_into().ok()?) as usize;
    let height = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
    let values: Vec<f32> = bytes[8..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect();
    let l = values
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| rgb_to_oklab(*p)[0])
        .collect();
    Some((l, width, height))
}

fn measure_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("GTP_NOISE_MEASURE_DIR")?);
    dir.is_dir().then_some(dir)
}

fn strength_from_env() -> f32 {
    std::env::var("GTP_WAVELET_STRENGTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(NR_WAVELET_STRENGTH)
}

fn median(mut runs: Vec<Duration>) -> Duration {
    runs.sort();
    runs[runs.len() / 2]
}

type Method = (
    &'static str,
    Box<dyn Fn(&[f32], usize, usize, f32) -> Vec<f32>>,
);

#[test]
#[ignore = "measurement, not a check; needs GTP_NOISE_MEASURE_DIR"]
fn measure_grain_kept_on_real_crops() {
    let Some(dir) = measure_dir() else {
        eprintln!("GTP_NOISE_MEASURE_DIR is not set: no real crops measured");
        return;
    };
    let strength = strength_from_env();
    eprintln!("wavelet strength {strength}, shares {NR_WAVELET_SHARES:?}");
    let mut stocks: Vec<(String, Vec<Crop>)> = Vec::new();
    for stock in ["ektar100", "vision3-250d", "vision3-500t"] {
        let crops: Vec<_> = ["flat", "a", "b", "c"]
            .iter()
            .filter_map(|t| load_crop(&dir.join(format!("{stock}-{t}.crop"))))
            .collect();
        if !crops.is_empty() {
            stocks.push((stock.to_string(), crops));
        }
    }
    // "Pushed": the 500T crops plus seeded grain whose energy in the scales 2 to 4 equals the
    // crop's own, so the coarse-grain energy about doubles. The grain is white noise plus the same
    // smoothed by the a trous cascade through 2, 3 and 4 levels, at equal deviations, scaled as said.
    if let Some((_, crops)) = stocks.iter().find(|(s, _)| s == "vision3-500t") {
        let pushed = crops
            .iter()
            .enumerate()
            .map(|(i, (l, w, h))| {
                let own = coarse(&energies(l, *w, *h));
                let base = grain(*w, *h, 900 + i as u32, 1.0, &[2, 3, 4]);
                let theirs = coarse(&energies(&base, *w, *h));
                let gain = (own / theirs).sqrt() as f32;
                let added: Vec<f32> = l.iter().zip(&base).map(|(l, g)| l + g * gain).collect();
                (added, *w, *h)
            })
            .collect();
        stocks.push(("pushed (500T + synthetic grain)".to_string(), pushed));
    }
    for (name, crops) in &stocks {
        let before: Vec<_> = crops.iter().map(|(l, w, h)| energies(l, *w, *h)).collect();
        eprintln!(
            "\n== {name}: {} crops of {}x{}",
            crops.len(),
            crops[0].1,
            crops[0].2
        );
        let totals: Vec<String> = (0..5)
            .map(|j| {
                let mean = before.iter().map(|e| e[j]).sum::<f64>() / before.len() as f64;
                format!("{:.2}", mean * 1e6)
            })
            .collect();
        eprintln!("input energy per scale (x1e-6): {}", totals.join(" "));
        let methods: [Method; 2] = [
            ("guided ", Box::new(guided_luminance)),
            (
                "wavelet",
                Box::new(move |l, w, h, a| wavelet_luminance(l, w, h, a, strength)),
            ),
        ];
        let mut curves = Vec::new();
        for (label, run) in &methods {
            eprintln!(
                "{label} amount  scale0  scale1  scale2  scale3  scale4 | fine kept  coarse kept"
            );
            let mut curve = vec![(0.0_f32, 1.0, 1.0)];
            let mut first_half = None;
            let beyond = [125.0_f32, 150.0, 200.0, 300.0, 400.0];
            for amount in (5..=100).step_by(5).map(|a| a as f32).chain(beyond) {
                let after: Vec<_> = crops
                    .iter()
                    .map(|(l, w, h)| energies(&run(l, *w, *h, amount), *w, *h))
                    .collect();
                let (fine, coarse_kept) = kept(&before, &after);
                curve.push((amount, fine, coarse_kept));
                if coarse_kept <= 0.5 && first_half.is_none() {
                    first_half = Some((amount, fine));
                }
                if AMOUNTS.contains(&amount) && amount <= 100.0 {
                    let per: Vec<String> = (0..5)
                        .map(|j| {
                            let b: f64 = before.iter().map(|e| e[j]).sum();
                            let a: f64 = after.iter().map(|e| e[j]).sum();
                            format!("{:6.3}", a / b)
                        })
                        .collect();
                    eprintln!(
                        "{label} {amount:6.0}  {} | {fine:9.3}  {coarse_kept:11.3}",
                        per.join("  ")
                    );
                }
            }
            curves.push(curve.clone());
            match first_half {
                Some((a, f)) => eprintln!(
                    "{label} lowest amount (steps of 5) with coarse kept <= 0.5: {a}, fine kept there {f:.3}"
                ),
                None => eprintln!("{label} coarse kept never reaches 0.5 up to 100"),
            }
            match fine_at_half_coarse(&curve) {
                Some((a, f)) => eprintln!(
                    "{label} interpolated: coarse kept = 0.5 at amount {a:.1}, fine kept {f:.3}"
                ),
                None => eprintln!("{label} interpolated: coarse kept does not cross 0.5"),
            }
        }
        // Equal coarse reduction: the coarse figure the guided filter reaches at its strongest
        // setting, and what each method keeps of the finest scale there (amounts beyond 100 are
        // not on the slider; they say how far the wavelet would have to go).
        let at_100 = curves[0].iter().find(|c| c.0 == 100.0).copied();
        if let Some((_, guided_fine, guided_coarse)) = at_100 {
            let reach = curves[1].windows(2).find_map(|pair| {
                let ((a0, f0, c0), (a1, f1, c1)) = (pair[0], pair[1]);
                (c0 > guided_coarse && c1 <= guided_coarse).then(|| {
                    let t = (guided_coarse - c0) / (c1 - c0);
                    (a0 + (a1 - a0) * t as f32, f0 + (f1 - f0) * t)
                })
            });
            match reach {
                Some((a, f)) => eprintln!(
                    "equal coarse ({guided_coarse:.3}, the guided filter at 100): guided keeps fine {guided_fine:.3}; wavelet keeps fine {f:.3} (reaches it at amount {a:.0})"
                ),
                None => eprintln!("equal coarse ({guided_coarse:.3}): wavelet does not reach it by amount 400"),
            }
        }
    }
}

#[test]
#[ignore = "measurement, not a check"]
fn measure_step_edge_sharpness() {
    let (width, height, edge) = (512_usize, 256_usize, 256_usize);
    let (low, high) = (0.3_f32, 0.7_f32);
    let noise = grain(width, height, 5, 0.012, &[2, 3]);
    let l: Vec<f32> = (0..width * height)
        .map(|i| if i % width >= edge { high } else { low } + noise[i])
        .collect();
    // The 10 to 90 percent rise width of the mean profile over the rows, between the plateaus.
    let rise = |plane: &[f32]| {
        let profile: Vec<f32> = (0..width)
            .map(|x| (0..height).map(|y| plane[y * width + x]).sum::<f32>() / height as f32)
            .collect();
        let plateau = |range: std::ops::Range<usize>| {
            range.clone().map(|x| profile[x]).sum::<f32>() / range.len() as f32
        };
        let (a, b) = (plateau(32..edge - 32), plateau(edge + 32..width - 32));
        let at = |fraction: f32| {
            let target = a + (b - a) * fraction;
            (1..width)
                .find(|x| profile[*x - 1] < target && profile[*x] >= target)
                .map(|x| {
                    let (p, q) = (profile[x - 1], profile[x]);
                    x as f32 - 1.0 + (target - p) / (q - p)
                })
                .unwrap_or(f32::NAN)
        };
        at(0.9) - at(0.1)
    };
    eprintln!("input (unfiltered) 10-90 rise width: {:.2} px", rise(&l));
    let strength = strength_from_env();
    for amount in AMOUNTS {
        let g = guided_luminance(&l, width, height, amount);
        let w = wavelet_luminance(&l, width, height, amount, strength);
        eprintln!(
            "amount {amount:3.0}: guided {:.2} px, wavelet {:.2} px",
            rise(&g),
            rise(&w)
        );
    }
}

#[test]
#[ignore = "measurement, not a check"]
fn measure_calibration_on_a_synthetic_grain_patch() {
    let (width, height) = (768_usize, 512_usize);
    let noise = grain(width, height, 11, 0.006, &[2, 3, 4]);
    let l: Vec<f32> = noise.iter().map(|n| 0.3 + n).collect();
    let before = energies(&l, width, height);
    let coarse_after = |plane: &[f32]| coarse(&energies(plane, width, height)) / coarse(&before);
    let fine_after = |plane: &[f32]| energies(plane, width, height)[0] / before[0];
    let guided: Vec<(f32, f64, f64)> = [25.0, 50.0, 100.0]
        .iter()
        .map(|a| {
            let g = guided_luminance(&l, width, height, *a);
            (*a, fine_after(&g), coarse_after(&g))
        })
        .collect();
    for (a, f, c) in &guided {
        eprintln!("guided  amount {a:3.0}: fine kept {f:.3}, coarse kept {c:.3}");
    }
    let mut best = (f64::MAX, 0.0);
    for hundredth in (10..=300).step_by(5) {
        let strength = hundredth as f32 / 100.0;
        let mut cost = 0.0;
        let mut rows = Vec::new();
        for (a, _, c) in &guided {
            let w = wavelet_luminance(&l, width, height, *a, strength);
            let (f2, c2) = (fine_after(&w), coarse_after(&w));
            cost += (c2.max(1e-6).ln() - c.max(1e-6).ln()).powi(2);
            rows.push((*a, f2, c2));
        }
        if cost < best.0 {
            best = (cost, strength);
        }
        if hundredth % 10 == 0 && hundredth <= 100 || hundredth % 50 == 0 {
            eprintln!("strength {strength:.1}: cost {cost:.3}; {rows:.3?}");
        }
    }
    eprintln!("best strength {} (cost {:.4})", best.1, best.0);
    let strength = strength_from_env();
    for (a, fg, cg) in &guided {
        let w = wavelet_luminance(&l, width, height, *a, strength);
        eprintln!(
            "amount {a:3.0}: guided coarse kept {cg:.3} fine {fg:.3} | wavelet (strength {strength}) coarse kept {:.3} fine {:.3}",
            coarse_after(&w),
            fine_after(&w)
        );
    }
}

#[test]
#[ignore = "timing only"]
fn measure_luminance_cost_at_24_megapixels() {
    let (width, height) = (6000_usize, 4000_usize);
    let mut next = stream_for_measure(77);
    let l: Vec<f32> = (0..width * height)
        .map(|_| 0.5 + gauss_for_measure(&mut next, 0.02))
        .collect();
    for threads in [12_usize, 6] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let run = |wavelet: bool| {
            median(
                (0..5)
                    .map(|_| {
                        let mut work = l.clone();
                        let started = Instant::now();
                        if wavelet {
                            wavelet_luminance_in(
                                &mut Workspace::default(),
                                &mut work,
                                (width, height),
                                50.0,
                                1.0,
                                NR_WAVELET_STRENGTH,
                            );
                        } else {
                            work = guided_luminance(&work, width, height, 50.0);
                        }
                        let elapsed = started.elapsed();
                        std::hint::black_box(work);
                        elapsed
                    })
                    .collect(),
            )
        };
        pool.install(|| {
            let g = run(false);
            let w = run(true);
            eprintln!(
                "luminance of 6000 x 4000 at 50, {threads} threads: guided {g:?}, wavelet {w:?} (cold workspace, copy of the plane made before the clock starts)"
            );
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Candidate strengths against a measure that does not use the a trous transform.
//
// The independent measure takes box means of the lightness (window `2r + 1` square, the window
// shrinking at the picture edge) and the energy of differences of box means: fine band
// `L - box1(L)` (3 x 3 box, so features of about 1 to 3 pixels) and coarse band
// `box1(L) - box12(L)` (3 x 3 against 25 x 25, features of about 3 to 25 pixels). Energy is the
// mean square over the picture without a border of 64 pixels.

const INDEPENDENT_FINE_RADIUS: usize = 1;
const INDEPENDENT_COARSE_RADIUS: usize = 12;
/// Strengths compared (shares unchanged).
const CANDIDATES: [f32; 4] = [0.3, 0.6, 0.9, 1.2];
/// Amounts of the curves the halving points are interpolated from.
const CURVE_AMOUNTS: [f32; 8] = [12.5, 25.0, 37.5, 50.0, 62.5, 75.0, 87.5, 100.0];

/// Box mean with a window of `2r + 1` square, clipped at the edges, from prefix sums.
fn plain_box_mean(src: &[f32], width: usize, height: usize, r: usize) -> Vec<f32> {
    let stride = width + 1;
    let mut rows = vec![0.0_f64; stride * height];
    for y in 0..height {
        for x in 0..width {
            rows[y * stride + x + 1] = rows[y * stride + x] + src[y * width + x] as f64;
        }
    }
    let mut cols = vec![0.0_f64; stride * (height + 1)];
    for y in 0..height {
        for x in 0..=width {
            cols[(y + 1) * stride + x] = cols[y * stride + x] + rows[y * stride + x];
        }
    }
    let at = |x: usize, y: usize| cols[y * stride + x];
    let mut out = vec![0.0_f32; src.len()];
    for y in 0..height {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(height));
        for x in 0..width {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(width));
            let sum = at(x1, y1) - at(x0, y1) - at(x1, y0) + at(x0, y0);
            out[y * width + x] = (sum / ((x1 - x0) * (y1 - y0)) as f64) as f32;
        }
    }
    out
}

/// (fine, coarse) energies of the box-difference bands of `l`.
fn box_energies(l: &[f32], width: usize, height: usize) -> [f64; 2] {
    let near = plain_box_mean(l, width, height, INDEPENDENT_FINE_RADIUS);
    let far = plain_box_mean(l, width, height, INDEPENDENT_COARSE_RADIUS);
    let m = reference::ENERGY_MARGIN;
    let (mut fine, mut coarse, mut n) = (0.0_f64, 0.0_f64, 0_usize);
    for y in m..height - m {
        for x in m..width - m {
            let i = y * width + x;
            fine += ((l[i] - near[i]) as f64).powi(2);
            coarse += ((near[i] - far[i]) as f64).powi(2);
            n += 1;
        }
    }
    [fine / n as f64, coarse / n as f64]
}

/// The crops of the three stocks and the pushed stand-in (see `measure_grain_kept_on_real_crops`).
fn load_stocks(dir: &Path) -> Vec<(String, Vec<Crop>)> {
    let mut stocks: Vec<(String, Vec<Crop>)> = Vec::new();
    for stock in ["ektar100", "vision3-250d", "vision3-500t"] {
        let crops: Vec<_> = ["flat", "a", "b", "c"]
            .iter()
            .filter_map(|t| load_crop(&dir.join(format!("{stock}-{t}.crop"))))
            .collect();
        if !crops.is_empty() {
            stocks.push((stock.to_string(), crops));
        }
    }
    if let Some((_, crops)) = stocks.iter().find(|(s, _)| s == "vision3-500t") {
        let pushed = crops
            .iter()
            .enumerate()
            .map(|(i, (l, w, h))| {
                let own = coarse(&energies(l, *w, *h));
                let base = grain(*w, *h, 900 + i as u32, 1.0, &[2, 3, 4]);
                let theirs = coarse(&energies(&base, *w, *h));
                let gain = (own / theirs).sqrt() as f32;
                let added: Vec<f32> = l.iter().zip(&base).map(|(l, g)| l + g * gain).collect();
                (added, *w, *h)
            })
            .collect();
        stocks.push(("pushed".to_string(), pushed));
    }
    stocks
}

type BothEnergies = ([f64; 5], [f64; 2]);

/// Energies summed over the planes: the a trous scales and the two box bands.
fn both_energies(planes: &[Crop]) -> BothEnergies {
    let per: Vec<BothEnergies> = planes
        .par_iter()
        .map(|(l, w, h)| (energies(l, *w, *h), box_energies(l, *w, *h)))
        .collect();
    let mut a = [0.0; 5];
    let mut b = [0.0; 2];
    for (ea, eb) in per {
        a.iter_mut().zip(ea).for_each(|(s, v)| *s += v);
        b.iter_mut().zip(eb).for_each(|(s, v)| *s += v);
    }
    (a, b)
}

/// Kept fractions: a trous fine, a trous coarse, box fine, box coarse.
fn kept4(before: &BothEnergies, after: &BothEnergies) -> [f64; 4] {
    [
        after.0[0] / before.0[0],
        (after.0[2] + after.0[3] + after.0[4]) / (before.0[2] + before.0[3] + before.0[4]),
        after.1[0] / before.1[0],
        after.1[1] / before.1[1],
    ]
}

/// Amount (and the fine figure there) where the coarse figure first crosses 0.5, interpolated
/// linearly over the curve that starts at amount 0 with everything kept.
fn halving(curve: &[(f32, [f64; 4])], fine: usize, coarse_ix: usize) -> Option<(f32, f64)> {
    let mut prev = (0.0_f32, 1.0_f64, 1.0_f64);
    for (a, k) in curve {
        let now = (*a, k[fine], k[coarse_ix]);
        if prev.2 > 0.5 && now.2 <= 0.5 {
            let t = (0.5 - prev.2) / (now.2 - prev.2);
            return Some((
                prev.0 + (now.0 - prev.0) * t as f32,
                prev.1 + (now.1 - prev.1) * t,
            ));
        }
        prev = now;
    }
    None
}

fn show_halving(h: Option<(f32, f64)>) -> String {
    match h {
        Some((a, f)) => format!("amount {a:5.1}, fine kept {f:.3}"),
        None => "not reached by 100".to_string(),
    }
}

type Run = Box<dyn Fn(&[f32], usize, usize, f32) -> Vec<f32>>;

#[test]
#[ignore = "measurement, not a check; needs GTP_NOISE_MEASURE_DIR"]
fn measure_candidate_strengths_on_real_crops() {
    let Some(dir) = measure_dir() else {
        eprintln!("GTP_NOISE_MEASURE_DIR is not set");
        return;
    };
    let only = std::env::var("GTP_MEASURE_ONLY").ok();
    for (name, crops) in load_stocks(&dir) {
        if only.as_ref().is_some_and(|o| !name.contains(o.as_str())) {
            continue;
        }
        let before = both_energies(&crops);
        eprintln!("\n== {name}");
        let mut methods: Vec<(String, Run)> = vec![("guided".into(), Box::new(guided_luminance))];
        for s in CANDIDATES {
            methods.push((
                format!("wav {s:.1}"),
                Box::new(move |l, w, h, a| wavelet_luminance(l, w, h, a, s)),
            ));
        }
        for (label, run) in &methods {
            let mut curve = Vec::new();
            for amount in CURVE_AMOUNTS {
                let after: Vec<Crop> = crops
                    .iter()
                    .map(|(l, w, h)| (run(l, *w, *h, amount), *w, *h))
                    .collect();
                curve.push((amount, kept4(&before, &both_energies(&after))));
            }
            for (amount, k) in &curve {
                if AMOUNTS.contains(amount) {
                    eprintln!(
                        "{label:8} {amount:4.0}: a trous fine {:.3} coarse {:.3} | box fine {:.3} coarse {:.3}",
                        k[0], k[1], k[2], k[3]
                    );
                }
            }
            eprintln!(
                "{label:8} coarse halves (a trous): {}; (box): {}",
                show_halving(halving(&curve, 0, 1)),
                show_halving(halving(&curve, 2, 3))
            );
        }
    }
}

fn deviation_of(plane: &[f32]) -> f32 {
    let mean = plane.iter().sum::<f32>() / plane.len() as f32;
    (plane.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / plane.len() as f32).sqrt()
}

/// The 10 to 90 percent rise width of the mean profile over the rows, between the plateaus.
fn rise_width(plane: &[f32], width: usize, height: usize, edge: usize) -> f32 {
    let profile: Vec<f32> = (0..width)
        .map(|x| (0..height).map(|y| plane[y * width + x]).sum::<f32>() / height as f32)
        .collect();
    let plateau = |range: std::ops::Range<usize>| {
        range.clone().map(|x| profile[x]).sum::<f32>() / range.len() as f32
    };
    let (a, b) = (plateau(32..edge - 32), plateau(edge + 32..width - 32));
    let at = |fraction: f32| {
        let target = a + (b - a) * fraction;
        (1..width)
            .find(|x| profile[*x - 1] < target && profile[*x] >= target)
            .map(|x| {
                let (p, q) = (profile[x - 1], profile[x]);
                x as f32 - 1.0 + (target - p) / (q - p)
            })
            .unwrap_or(f32::NAN)
    };
    at(0.9) - at(0.1)
}

#[test]
#[ignore = "measurement, not a check"]
fn measure_candidate_strengths_on_synthetic_patches() {
    // White noise, 256 x 192, deviation 0.01 around 0.3.
    let (ww, wh) = (256_usize, 192_usize);
    let mut next = stream_for_measure(31);
    let white: Vec<f32> = (0..ww * wh)
        .map(|_| 0.3 + gauss_for_measure(&mut next, 0.01))
        .collect();
    let before_dev = deviation_of(&white);
    eprintln!("white noise patch {ww}x{wh}: deviation {before_dev:.5}");
    eprintln!(
        "  guided at 100: {:.3} of the deviation kept",
        deviation_of(&guided_luminance(&white, ww, wh, 100.0)) / before_dev
    );
    for s in CANDIDATES {
        let out = wavelet_luminance(&white, ww, wh, 100.0, s);
        eprintln!(
            "  wavelet {s:.1} at 100: {:.3} of the deviation kept",
            deviation_of(&out) / before_dev
        );
    }

    // Step edge with grain, as measure_step_edge_sharpness.
    let (width, height, edge) = (512_usize, 256_usize, 256_usize);
    let noise = grain(width, height, 5, 0.012, &[2, 3]);
    let l: Vec<f32> = (0..width * height)
        .map(|i| if i % width >= edge { 0.7 } else { 0.3 } + noise[i])
        .collect();
    eprintln!(
        "step edge input rise {:.2} px",
        rise_width(&l, width, height, edge)
    );
    for amount in [50.0_f32, 100.0] {
        let guided = guided_luminance(&l, width, height, amount);
        let mut line = format!(
            "  amount {amount:3.0}: guided {:.2}",
            rise_width(&guided, width, height, edge)
        );
        for s in CANDIDATES {
            let out = wavelet_luminance(&l, width, height, amount, s);
            line += &format!(" | wav {s:.1} {:.2}", rise_width(&out, width, height, edge));
        }
        eprintln!("{line} px");
    }

    // The flat-grain calibration patch.
    let (width, height) = (768_usize, 512_usize);
    let noise = grain(width, height, 11, 0.006, &[2, 3, 4]);
    let l: Vec<f32> = noise.iter().map(|n| 0.3 + n).collect();
    let before = both_energies(&[(l.clone(), width, height)]);
    eprintln!("flat grain patch (kept: a trous fine, coarse | box fine, coarse)");
    for amount in AMOUNTS {
        let mut rows = vec![(
            "guided".to_string(),
            guided_luminance(&l, width, height, amount),
        )];
        for s in CANDIDATES {
            rows.push((
                format!("wav {s:.1}"),
                wavelet_luminance(&l, width, height, amount, s),
            ));
        }
        for (label, out) in rows {
            let k = kept4(&before, &both_energies(&[(out, width, height)]));
            eprintln!(
                "  {label:8} {amount:4.0}: {:.3} {:.3} | {:.3} {:.3}",
                k[0], k[1], k[2], k[3]
            );
        }
    }
}
