//! Figures for decision 0019 (colours above white). Not checks: each test is ignored and prints
//! its table; run them with
//! `cargo test --release -p gtp-app measure_ -- --ignored --nocapture --test-threads=1`.
//!
//! Three rules bring a linear Rec.2020 pixel into `[0, 1]`:
//! - A: clip each channel on its own (the rule before P-32);
//! - B: scale by the largest channel so it is 1 (hue kept, no roll-off);
//! - C(K): scale by the largest channel, then blend towards white by `(m - 1) / (m - 1 + K)`.
//!
//! Hue shift and chroma change are measured in Oklab on what the screen shows (after the matrix and
//! the clip to sRGB), against the source colour scaled so its largest channel is 1 (its true hue).
//! Pixels whose true chroma is under [`MIN_TRUE_CHROMA`], or whose result has under
//! [`MIN_RESULT_CHROMA`], have no stable hue and are left out of the hue figures (the count is
//! printed).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gtp_io::{read_tiff16, split_rgbi, to_f32_linear, Limits};

use super::against_export::strong_render;
use super::*;
use crate::pipeline::{analyze, render, PipelineInput, RenderTarget};
use gtp_core::testing::synthetic::Pattern;

/// Oklab chroma below which the source colour has no hue worth measuring.
const MIN_TRUE_CHROMA: f32 = 0.02;
/// Oklab chroma below which a result has no stable hue.
const MIN_RESULT_CHROMA: f32 = 0.005;
/// Long edge of the real renders, the size of the app's preview.
const REAL_PREVIEW_EDGE: u32 = 1600;

#[derive(Clone, Copy)]
enum Rule {
    Clip,
    Scale,
    Soft(f32),
}

impl Rule {
    fn name(self) -> String {
        match self {
            Rule::Clip => "A clip".to_string(),
            Rule::Scale => "B scale".to_string(),
            Rule::Soft(k) => format!("C K={k}"),
        }
    }

    fn apply(self, rgb: [f32; 3]) -> [f32; 3] {
        let p = rgb.map(|v| {
            if v.is_nan() {
                0.0
            } else {
                v.clamp(0.0, f32::MAX)
            }
        });
        let m = p[0].max(p[1]).max(p[2]);
        if m <= 1.0 {
            return p;
        }
        match self {
            Rule::Clip => p.map(|v| v.clamp(0.0, 1.0)),
            Rule::Scale => p.map(|v| (v / m).clamp(0.0, 1.0)),
            Rule::Soft(k) => {
                let over = m - 1.0;
                let t = over / (over + k);
                p.map(|v| {
                    let c = v / m;
                    (c + (1.0 - c) * t).clamp(0.0, 1.0)
                })
            }
        }
    }
}

const RULES: [Rule; 5] = [
    Rule::Clip,
    Rule::Scale,
    Rule::Soft(0.5),
    Rule::Soft(1.0),
    Rule::Soft(2.0),
];

/// Linear sRGB of a pixel already in range, clipped as the screen does.
fn shown(rgb: [f32; 3]) -> [f32; 3] {
    REC2020_TO_SRGB.map(|row| (row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2]).clamp(0.0, 1.0))
}

fn bytes(rgb: [f32; 3], thresholds: &[f32; 255]) -> [u8; 3] {
    shown(rgb).map(|v| byte_of(v, thresholds))
}

/// Oklab (L, a, b) of a linear sRGB colour.
fn oklab(c: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = c;
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn chroma(lab: [f32; 3]) -> f32 {
    lab[1].hypot(lab[2])
}

/// The source scaled so its largest channel is 1: the colour with its hue at full brightness.
fn true_colour(rgb: [f32; 3]) -> [f32; 3] {
    let p = rgb.map(|v| {
        if v.is_nan() {
            0.0
        } else {
            v.clamp(0.0, f32::MAX)
        }
    });
    let m = p[0].max(p[1]).max(p[2]);
    if m > 0.0 {
        p.map(|v| v / m)
    } else {
        p
    }
}

/// Signed hue difference in degrees between two Oklab colours.
fn hue_shift(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = (b[2].atan2(b[1]) - a[2].atan2(a[1])).to_degrees();
    (d + 180.0).rem_euclid(360.0) - 180.0
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return f32::NAN;
    }
    sorted[((sorted.len() - 1) as f32 * p).round() as usize]
}

#[test]
#[ignore = "measurement, not a check"]
fn measure_rules_on_test_colours() {
    let thresholds = level_thresholds();
    let named: [(&str, [f32; 3]); 4] = [
        ("orange", [1.0, 0.5, 0.11]),
        ("green", [0.22, 1.0, 0.33]),
        ("magenta", [1.0, 0.17, 0.83]),
        ("blue", [0.1, 0.4, 1.0]),
    ];
    let mut colours: Vec<(String, [f32; 3])> = vec![
        ("orange (1.8,0.9,0.2)".into(), [1.8, 0.9, 0.2]),
        ("green (0.4,1.8,0.6)".into(), [0.4, 1.8, 0.6]),
        ("green (0.6,2.7,0.9)".into(), [0.6, 2.7, 0.9]),
        ("magenta (1.8,0.3,1.5)".into(), [1.8, 0.3, 1.5]),
    ];
    for (name, base) in named {
        for k in [1.0f32, 2.0, 3.0] {
            colours.push((format!("{name} x{k}"), base.map(|v| v * k)));
        }
    }
    eprintln!("colour; rule; sRGB bytes; hue shift (deg); chroma change (Oklab, shown minus true)");
    for (label, rgb) in colours {
        let truth = oklab(shown(true_colour(rgb)));
        for rule in RULES {
            let out = rule.apply(rgb);
            let lab = oklab(shown(out));
            let [r, g, b] = bytes(out, thresholds);
            eprintln!(
                "{label:24} {:9} ({r:3},{g:3},{b:3})  hue {:+7.2}  chroma {:+.3}  (true chroma {:.3})",
                rule.name(),
                hue_shift(truth, lab),
                chroma(lab) - chroma(truth),
                chroma(truth)
            );
        }
    }
}

/// Prints, per rule, the figures over the pixels of `image` with a channel above 1.
fn report_image(label: &str, image: &ImageF32) {
    let pixels = image.data.as_chunks::<3>().0;
    let above: Vec<[f32; 3]> = pixels
        .iter()
        .copied()
        .filter(|p| p.iter().any(|v| *v > 1.0))
        .collect();
    eprintln!(
        "{label}: {} of {} pixels above 1 ({:.1} %)",
        above.len(),
        pixels.len(),
        100.0 * above.len() as f64 / pixels.len().max(1) as f64
    );
    if above.is_empty() {
        return;
    }
    for rule in RULES {
        let mut shifts = Vec::new();
        let mut lightness = 0.0f64;
        for p in &above {
            let truth = oklab(shown(true_colour(*p)));
            let lab = oklab(shown(rule.apply(*p)));
            lightness += f64::from(lab[0]);
            if chroma(truth) >= MIN_TRUE_CHROMA && chroma(lab) >= MIN_RESULT_CHROMA {
                shifts.push(hue_shift(truth, lab).abs());
            }
        }
        shifts.sort_by(f32::total_cmp);
        let mean = shifts.iter().map(|v| f64::from(*v)).sum::<f64>() / shifts.len().max(1) as f64;
        eprintln!(
            "    {:9} hue shift mean {mean:6.2} deg, p99 {:6.2} deg ({} pixels with a hue); mean Oklab L {:.4}",
            rule.name(),
            percentile(&shifts, 0.99),
            shifts.len(),
            lightness / above.len() as f64
        );
    }
}

#[test]
#[ignore = "measurement, not a check"]
fn measure_rules_on_synthetic_renders() {
    for (pattern, seed) in [(Pattern::Gradient, 1u64), (Pattern::Blobs, 3)] {
        for ev in [0.0f32, 1.5] {
            report_image(
                &format!("{pattern:?} at {ev} EV, strong saturation"),
                &strong_render(pattern, seed, ev),
            );
        }
    }
}

/// The scans in `samples/private` (git-ignored) used for the real renders.
const REAL_SCANS: [&str; 5] = [
    "Ektar100 C41 0001.tif",
    "Vision3 250D ECN2 0001.tif",
    "Ektacolor400 C41 0001.tif",
    "Ektacolor160 C41 0001.tif",
    "Vision3 500T 0001.tif",
];

#[test]
#[ignore = "measurement, not a check; needs samples/private"]
fn measure_rules_on_real_renders() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/private");
    if !dir.is_dir() {
        eprintln!("samples/private is missing: no real renders measured");
        return;
    }
    for name in REAL_SCANS {
        let path = dir.join(name);
        if !path.is_file() {
            eprintln!("{name}: not found");
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
        let target = RenderTarget::Preview {
            max_edge: REAL_PREVIEW_EDGE,
        };
        report_image(
            &format!("{name}, automatic recipe"),
            &render(&input, &auto, target).unwrap(),
        );
        let mut strong = auto.clone();
        strong.color.saturation = 2.0;
        strong.color.vibrance = 100.0;
        strong.color.mixer.saturation = [100.0; 8];
        report_image(
            &format!("{name}, saturation strong"),
            &render(&input, &strong, target).unwrap(),
        );
    }
}

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

/// `to_rgba8_srgb` with another rule in place of `bring_into_range`: the old clip (A) or the
/// scale to 1 (B), timed against the real conversion.
fn rule_to_rgba8_srgb(image: &ImageF32, rule: Rule) -> Vec<u8> {
    let thresholds = level_thresholds();
    let (pixels, _) = image.data.as_chunks::<3>();
    let mut out = vec![0u8; pixels.len() * 4];
    out.par_chunks_mut(PIXELS_PER_TASK * 4)
        .zip(pixels.par_chunks(PIXELS_PER_TASK))
        .for_each(|(rgba_run, rgb_run)| {
            let (rgba_run, _) = rgba_run.as_chunks_mut::<4>();
            for (rgba, rgb) in rgba_run.iter_mut().zip(rgb_run) {
                let rgb = rule.apply(*rgb);
                for (slot, row) in rgba.iter_mut().zip(REC2020_TO_SRGB) {
                    let linear = row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2];
                    *slot = byte_of(linear.clamp(0.0, 1.0), thresholds);
                }
                rgba[3] = 255;
            }
        });
    out
}

/// A noisy ramp whose brightest `bright_share` of columns go above 1 (up to about 3).
fn timing_image(w: u32, h: u32, bright_share: f32) -> ImageF32 {
    let mut state = 0x9E37_79B9u32;
    let data = (0..w as usize * h as usize * 3)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let x = (i / 3 % w as usize) as f32 / w as f32;
            let base = x * 0.9 + (state % 1000) as f32 / 10_000.0;
            if bright_share > 0.0 && x > 1.0 - bright_share && i % 3 != 2 {
                base * 3.0
            } else {
                base
            }
        })
        .collect();
    ImageF32::new(w, h, 3, data).unwrap()
}

/// Median of seven runs of `to_rgba8_srgb`, the new rule against the old clip and the scale to 1,
/// at preview size.
#[test]
#[ignore = "timing run, not a check"]
fn measure_time_screen_conversion() {
    eprintln!(
        "rayon threads in the global pool: {}",
        rayon::current_num_threads()
    );
    for (label, share) in [
        ("no pixel above 1", 0.0f32),
        ("half the pixels above 1", 0.5),
    ] {
        let image = timing_image(2048, 1365, share);
        let new = time(&|| {
            to_rgba8_srgb(&image).unwrap();
        });
        let old = time(&|| {
            rule_to_rgba8_srgb(&image, Rule::Clip);
        });
        let scale = time(&|| {
            rule_to_rgba8_srgb(&image, Rule::Scale);
        });
        eprintln!("2048x1365, {label}: new rule {new:?}, old clip {old:?}, scale to 1 {scale:?}");
    }
}

/// The figures are only worth something if rule C at the chosen knee is the rule the app uses.
#[test]
fn the_measured_soft_rule_is_the_rule_the_app_uses() {
    let values = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -1.0,
        0.0,
        0.3,
        0.999_999,
        1.0,
        1.000_001,
        1.8,
        2.7,
        40.0,
        f32::MAX,
    ];
    for r in values {
        for g in values {
            for b in values {
                let measured = Rule::Soft(gtp_core::HIGHLIGHT_KNEE).apply([r, g, b]);
                let used = gtp_core::bring_into_range([r, g, b]);
                assert_eq!(
                    measured.map(f32::to_bits),
                    used.map(f32::to_bits),
                    "{r} {g} {b}"
                );
            }
        }
    }
}
