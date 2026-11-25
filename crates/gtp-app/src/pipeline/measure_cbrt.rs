//! Figures for decision 0022 (our own cube root). Not a check: the test is ignored. It renders a
//! fixed set of cases through the real pipeline and the real export conversions, and either writes
//! the finished values or compares them with an earlier write.
//!
//! The environment variable `GTP_CBRT_DIR` names a directory outside the repository and
//! `GTP_CBRT_MODE` is `dump` or `compare`. A dump writes, per case, the 16-bit export values in
//! sRGB, Adobe RGB and ProPhoto (`gtp_io::to_output`, the export's own conversion) and the 8-bit
//! screen bytes (`to_rgba8_srgb`, alpha dropped), as little-endian `u16` or bytes. A comparison
//! reads those files back and prints, per case, the number of values, the share that changed and
//! the largest change in 16-bit levels (per space) and in 8-bit levels. It ends with the largest
//! changes overall and marks every case outside the tolerance of the decision: no 8-bit value
//! changes by more than 1 level, and at most 0.02 % of the 16-bit values (per space) change by
//! more than 1 level. A comparison fails the test when a dump is missing or has another length, when
//! no case was compared, or when a case is outside the tolerance. The real scans of
//! `samples/private` (git-ignored) are read as in `measure_noise`; nothing from them is written into the repository. Run with
//! `cargo test --release -p gtp-app measure_cbrt -- --ignored --nocapture --test-threads=1`.
//!
//! The cases:
//! - synthetic negatives with grain (three scenes: a noisy gradient, dark blobs, saturated colours
//!   with values above 1), each rendered with noise only, the mixer only, and both, under a neutral
//!   tone, a steep tone (exposure, contrast and a steep curve) and a strong mixer (hue, saturation
//!   and lightness at +-100);
//! - two direct pictures (the stages applied to colour grids with seeded noise, with values above 1
//!   and below 0), which the pipeline never builds;
//! - the real scans: automatic recipe plus noise and mixer, normal and steep, at the size of the
//!   preview, and the "both" cases at full size.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gtp_core::recipe::{BaseSource, Crop, Invert, Mixer, Noise, OutputSpace, MIXER_BANDS};
use gtp_core::testing::synthetic::{self, Params, Pattern};
use gtp_core::{ImageF32, Recipe};
use gtp_io::{read_tiff16, split_rgbi, to_f32_linear, to_output, Limits};

use super::{analyze, render, to_rgba8_srgb, PipelineInput, RenderTarget};

/// Largest accepted change, in levels, of an 8-bit value. For a 16-bit value it is the change that
/// is not counted against [`MAX_SHARE_OVER_PERCENT`].
const TOLERANCE_LEVELS: u32 = 1;
/// Largest accepted share, in percent, of the 16-bit values of a space that change by more than
/// [`TOLERANCE_LEVELS`]. The limit was fitted to the measured results (decision 0022).
const MAX_SHARE_OVER_PERCENT: f64 = 0.02;
/// Long edge of the preview-size renders of the real scans.
const PREVIEW_EDGE: u32 = 1600;
/// The scans measured (as in `measure_noise`): fine grain, middle and coarse.
const STOCKS: [(&str, &str); 3] = [
    ("ektar100", "Ektar100 C41 0001.tif"),
    ("vision3-250d", "Vision3 250D ECN2 0001.tif"),
    ("vision3-500t", "Vision3 500T 0001.tif"),
];
/// The spaces of the dump, with the file tag of each.
const SPACES: [(&str, OutputSpace); 3] = [
    ("srgb", OutputSpace::Srgb),
    ("adobe", OutputSpace::AdobeRgb),
    ("prophoto", OutputSpace::Prophoto),
];

/// Which of the two stages that use the Oklab conversion are on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stages {
    NoiseOnly,
    MixerOnly,
    Both,
}

impl Stages {
    const ALL: [Stages; 3] = [Stages::NoiseOnly, Stages::MixerOnly, Stages::Both];

    fn name(self) -> &'static str {
        match self {
            Stages::NoiseOnly => "noise",
            Stages::MixerOnly => "mixer",
            Stages::Both => "both",
        }
    }

    fn noise(self) -> bool {
        self != Stages::MixerOnly
    }

    fn mixer(self) -> bool {
        self != Stages::NoiseOnly
    }
}

/// How hard the recipe pushes the picture.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Strength {
    /// Default tone, a moderate mixer and noise amounts of 50.
    Neutral,
    /// Exposure, contrast and a steep curve, a moderate mixer, noise amounts of 100.
    Steep,
    /// Default tone, a mixer at +-100 in every band, noise amounts of 100.
    StrongMixer,
}

impl Strength {
    fn name(self) -> &'static str {
        match self {
            Strength::Neutral => "neutral",
            Strength::Steep => "steep",
            Strength::StrongMixer => "strong-mixer",
        }
    }

    fn noise(self) -> Noise {
        let amount = if self == Strength::Neutral {
            50.0
        } else {
            100.0
        };
        Noise {
            luminance: amount,
            color: amount,
        }
    }

    fn mixer(self) -> Mixer {
        let alternate = |scale: f32, offset: usize| -> [f32; MIXER_BANDS] {
            let mut out = [0.0; MIXER_BANDS];
            for (i, v) in out.iter_mut().enumerate() {
                *v = if (i + offset).is_multiple_of(2) {
                    scale
                } else {
                    -scale
                };
            }
            out
        };
        let scale = if self == Strength::StrongMixer {
            100.0
        } else {
            50.0
        };
        Mixer {
            hue: alternate(scale, 0),
            saturation: alternate(scale, 1),
            lightness: alternate(scale, 0),
        }
    }

    /// The tone sliders of a steep recipe.
    fn steepen(self, recipe: &mut Recipe) {
        if self == Strength::Steep {
            recipe.tone.exposure_ev = 1.0;
            recipe.tone.contrast = 80.0;
            recipe.tone.highlights = -40.0;
            recipe.tone.shadows = 40.0;
            recipe.tone.curve.master =
                vec![[0.0, 0.0], [0.2, 0.05], [0.5, 0.5], [0.8, 0.95], [1.0, 1.0]];
        }
    }
}

/// Sets the stages and the strength on `recipe`.
fn apply_case(recipe: &mut Recipe, stages: Stages, strength: Strength) {
    recipe.noise = if stages.noise() {
        strength.noise()
    } else {
        Noise::default()
    };
    recipe.color.mixer = if stages.mixer() {
        strength.mixer()
    } else {
        Mixer::default()
    };
    strength.steepen(recipe);
}

/// What one case produced, as the finished values.
struct Finished {
    /// 16-bit values per space, in the order of [`SPACES`].
    export: Vec<Vec<u16>>,
    /// 8-bit screen bytes, RGB.
    screen: Vec<u8>,
}

fn finish(image: &ImageF32) -> Finished {
    let export = SPACES
        .iter()
        .map(|(_, space)| to_output(image, *space).unwrap().data)
        .collect();
    let screen = to_rgba8_srgb(image)
        .unwrap()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    Finished { export, screen }
}

/// Where a case goes: a dump or a comparison.
struct Sink {
    dir: PathBuf,
    compare: bool,
    rows: Vec<Row>,
}

/// One line of the comparison.
struct Row {
    name: String,
    values: usize,
    export: Vec<Change>,
    screen: Change,
}

/// How a finished plane changed.
#[derive(Clone, Copy, Default)]
struct Change {
    /// Values that changed at all.
    changed: usize,
    /// Values that changed by more than the tolerance.
    over: usize,
    /// The largest change in levels.
    worst: u32,
    /// The value before the change at the place of the largest change.
    old_at_worst: u32,
}

impl Row {
    fn exceeds(&self) -> bool {
        self.export
            .iter()
            .any(|e| share(e.over, self.values) > MAX_SHARE_OVER_PERCENT)
            || self.screen.worst > TOLERANCE_LEVELS
    }
}

fn write_u16(path: &Path, data: &[u16]) {
    let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(path, bytes).unwrap();
}

fn read_u16(path: &Path) -> Option<Vec<u16>> {
    let bytes = std::fs::read(path).ok()?;
    Some(
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect(),
    )
}

fn changes<T: Copy + Into<u32>>(old: &[T], new: &[T]) -> Change {
    let mut out = Change::default();
    for (a, b) in old.iter().zip(new) {
        let (a, b): (u32, u32) = ((*a).into(), (*b).into());
        if a != b {
            out.changed += 1;
            let d = a.abs_diff(b);
            if d > TOLERANCE_LEVELS {
                out.over += 1;
            }
            if d > out.worst {
                out.worst = d;
                out.old_at_worst = a;
            }
        }
    }
    out
}

impl Sink {
    fn record(&mut self, name: &str, finished: &Finished) {
        if !self.compare {
            for ((tag, _), data) in SPACES.iter().zip(&finished.export) {
                write_u16(&self.dir.join(format!("{name}.{tag}.u16")), data);
            }
            std::fs::write(self.dir.join(format!("{name}.screen.u8")), &finished.screen).unwrap();
            eprintln!("{name}: dumped {} values", finished.screen.len());
            return;
        }
        let mut export = Vec::new();
        for ((tag, _), new) in SPACES.iter().zip(&finished.export) {
            let Some(old) = read_u16(&self.dir.join(format!("{name}.{tag}.u16"))) else {
                panic!("{name}: no dump for {tag}");
            };
            assert_eq!(
                old.len(),
                new.len(),
                "{name}: {tag} has {} values, the dump {}",
                new.len(),
                old.len()
            );
            export.push(changes(&old, new));
        }
        let Ok(old) = std::fs::read(self.dir.join(format!("{name}.screen.u8"))) else {
            panic!("{name}: no dump for the screen");
        };
        assert_eq!(
            old.len(),
            finished.screen.len(),
            "{name}: the screen has a different length from the dump"
        );
        let row = Row {
            name: name.to_string(),
            values: finished.screen.len(),
            export,
            screen: changes(&old, &finished.screen),
        };
        print_row(&row);
        self.rows.push(row);
    }

    fn summary(&self) {
        if !self.compare {
            return;
        }
        assert!(!self.rows.is_empty(), "no case was compared");
        eprintln!("---- summary ({} cases)", self.rows.len());
        let mut worst16 = 0;
        let mut worst8 = 0;
        let mut changed_cases = 0;
        for row in &self.rows {
            worst16 = worst16.max(row.export.iter().map(|e| e.worst).max().unwrap_or(0));
            worst8 = worst8.max(row.screen.worst);
            if row.export.iter().any(|e| e.changed > 0) || row.screen.changed > 0 {
                changed_cases += 1;
            }
        }
        eprintln!(
            "largest change anywhere: {worst16} levels of 16 bits, {worst8} levels of 8 bits; \
             {changed_cases} of {} cases changed at all",
            self.rows.len()
        );
        let over: Vec<&str> = self
            .rows
            .iter()
            .filter(|r| r.exceeds())
            .map(|r| r.name.as_str())
            .collect();
        let share_over = self
            .rows
            .iter()
            .flat_map(|r| r.export.iter().map(|e| share(e.over, r.values)))
            .fold(0.0, f64::max);
        eprintln!(
            "largest share of 16-bit values over {TOLERANCE_LEVELS} level: {share_over:.4} %"
        );
        assert!(
            over.is_empty(),
            "OUTSIDE THE TOLERANCE (8-bit at most {TOLERANCE_LEVELS} level, 16-bit share over {TOLERANCE_LEVELS} level at most {MAX_SHARE_OVER_PERCENT} %): {over:?}"
        );
        eprintln!(
            "every case is within the tolerance (8-bit at most {TOLERANCE_LEVELS} level, 16-bit share over {TOLERANCE_LEVELS} level at most {MAX_SHARE_OVER_PERCENT} %)"
        );
    }
}

fn share(changed: usize, total: usize) -> f64 {
    100.0 * changed as f64 / total.max(1) as f64
}

fn print_row(row: &Row) {
    let spaces: Vec<String> = SPACES
        .iter()
        .zip(&row.export)
        .map(|((tag, _), c)| {
            format!(
                "{tag} {:.4} % max {} (old value {}, {} over)",
                share(c.changed, row.values),
                c.worst,
                c.old_at_worst,
                c.over
            )
        })
        .collect();
    eprintln!(
        "{}: {} values | 16-bit: {} | 8-bit: {:.4} % max {}{}",
        row.name,
        row.values,
        spaces.join(", "),
        share(row.screen.changed, row.values),
        row.screen.worst,
        if row.exceeds() { "  EXCEEDS" } else { "" }
    );
}

/// The recipe that undoes the synthetic model exactly, with the white point moved by
/// `white_scale` (below 1 puts the bright part above 1).
fn true_recipe(s: &synthetic::Synthetic, white_scale: f32) -> Recipe {
    let (w, h) = (s.image.width as f32, s.image.height as f32);
    let f = s.truth.frame;
    let mut recipe = Recipe::default();
    recipe.geometry.crop = Crop {
        x: f.x as f32 / w,
        y: f.y as f32 / h,
        w: f.w as f32 / w,
        h: f.h as f32 / h,
    };
    recipe.base.source = BaseSource::Manual;
    recipe.base.rgb = s.truth.base;
    recipe.invert = Invert {
        black: s.truth.black,
        white: s.truth.white.map(|v| v * white_scale),
        combine: None,
    };
    recipe
}

/// The three synthetic scenes: name, seed, parameters and the white scale of the recipe.
fn scenes() -> Vec<(&'static str, u64, Params, f32)> {
    vec![
        (
            "gradient",
            11,
            Params {
                width: 640,
                height: 480,
                rebate: [24, 16, 32, 20],
                pattern: Pattern::Gradient,
                noise: 0.06,
                ..Params::default()
            },
            1.0,
        ),
        (
            "dark-blobs",
            23,
            Params {
                width: 640,
                height: 480,
                rebate: [24, 16, 32, 20],
                pattern: Pattern::Blobs,
                black: [0.45, 0.50, 0.55],
                white: [2.4, 2.5, 2.6],
                noise: 0.10,
                ..Params::default()
            },
            1.0,
        ),
        (
            "saturated-over-one",
            37,
            Params {
                width: 640,
                height: 480,
                rebate: [24, 16, 32, 20],
                pattern: Pattern::Wedge,
                black: [0.05, 0.30, 0.60],
                white: [1.2, 2.6, 3.6],
                noise: 0.08,
                ..Params::default()
            },
            0.7,
        ),
    ]
}

fn synthetic_cases(sink: &mut Sink) {
    for (scene, seed, params, white_scale) in scenes() {
        let s = synthetic::negative(seed, &params).unwrap();
        let input = PipelineInput {
            rgb: Arc::new(s.image.clone()),
            ir: None,
            film_base: None,
            full_long_edge: None,
        };
        for strength in [Strength::Neutral, Strength::Steep, Strength::StrongMixer] {
            for stages in Stages::ALL {
                // A strong mixer with the mixer off is the neutral noise-only case again.
                if strength == Strength::StrongMixer && stages == Stages::NoiseOnly {
                    continue;
                }
                let mut recipe = true_recipe(&s, white_scale);
                apply_case(&mut recipe, stages, strength);
                let image = render(&input, &recipe, RenderTarget::Full).unwrap();
                let over = image.data.iter().filter(|v| **v > 1.0).count();
                let name = format!("synth-{scene}-{}-{}", strength.name(), stages.name());
                eprintln!("{name}: {over} samples above 1");
                sink.record(&name, &finish(&image));
            }
        }
    }
}

/// A colour grid with values from below 0 to above 1 and seeded noise, as a picture.
fn direct_picture(width: u32, height: u32, seed: u32, lo: f32, hi: f32, noise: f32) -> ImageF32 {
    let mut state = seed;
    let mut next = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let u = x as f32 / (width - 1) as f32;
            let v = y as f32 / (height - 1) as f32;
            // Red along x, green along y, blue on a diagonal ramp, repeated in bands so that
            // dark, saturated and bright colours meet.
            let base = [u, v, ((u * 3.0 + v * 2.0) % 1.0).abs()];
            for b in base {
                let value = lo + (hi - lo) * b + noise * (next() - 0.5);
                data.push(value);
            }
        }
    }
    ImageF32::new(width, height, 3, data).unwrap()
}

fn direct_cases(sink: &mut Sink) {
    // (name, lo, hi, noise): in range with grain, and from below 0 to well above 1.
    let pictures = [
        ("direct-in-range", 0.0, 1.0, 0.04),
        ("direct-out-of-range", -0.05, 3.0, 0.10),
    ];
    for (name, lo, hi, noise) in pictures {
        for strength in [Strength::Neutral, Strength::StrongMixer] {
            for stages in Stages::ALL {
                let mut image = direct_picture(512, 384, 7, lo, hi, noise);
                if stages.noise() {
                    gtp_edit::apply_noise_reduction(&mut image, &strength.noise(), 1.0).unwrap();
                }
                if stages.mixer() {
                    gtp_edit::apply_mixer(&mut image, &strength.mixer()).unwrap();
                }
                let label = format!("{name}-{}-{}", strength.name(), stages.name());
                sink.record(&label, &finish(&image));
            }
        }
    }
}

fn load(path: &Path) -> Arc<ImageF32> {
    let tiff = read_tiff16(path, &Limits::default()).unwrap();
    let rgb = if tiff.channels == 4 {
        split_rgbi(&tiff).unwrap().0
    } else {
        tiff
    };
    Arc::new(to_f32_linear(&rgb).unwrap())
}

fn real_cases(sink: &mut Sink) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/private");
    if !dir.is_dir() {
        eprintln!("samples/private is missing: the real scans are skipped");
        return;
    }
    for (stock, file) in STOCKS {
        let path = dir.join(file);
        if !path.is_file() {
            eprintln!("{file}: not found, skipped");
            continue;
        }
        let input = PipelineInput {
            rgb: load(&path),
            ir: None,
            film_base: None,
            full_long_edge: None,
        };
        let auto = analyze(&input).unwrap().auto_recipe;
        for strength in [Strength::Neutral, Strength::Steep] {
            for stages in Stages::ALL {
                let mut recipe = auto.clone();
                apply_case(&mut recipe, stages, strength);
                let target = RenderTarget::Preview {
                    max_edge: PREVIEW_EDGE,
                };
                let image = render(&input, &recipe, target).unwrap();
                let name = format!("real-{stock}-preview-{}-{}", strength.name(), stages.name());
                sink.record(&name, &finish(&image));
            }
            // The full-size render, with both stages, the case that matters for an export.
            let mut recipe = auto.clone();
            apply_case(&mut recipe, Stages::Both, strength);
            let image = render(&input, &recipe, RenderTarget::Full).unwrap();
            let name = format!("real-{stock}-full-{}-both", strength.name());
            sink.record(&name, &finish(&image));
        }
    }
}

#[test]
#[ignore = "measurement, not a check; needs GTP_CBRT_DIR and GTP_CBRT_MODE (dump or compare)"]
fn measure_cbrt_changes_in_finished_values() {
    let (Some(dir), Some(mode)) = (
        std::env::var_os("GTP_CBRT_DIR"),
        std::env::var("GTP_CBRT_MODE").ok(),
    ) else {
        eprintln!("GTP_CBRT_DIR or GTP_CBRT_MODE is not set: nothing done");
        return;
    };
    let compare = match mode.as_str() {
        "dump" => false,
        "compare" => true,
        other => panic!("GTP_CBRT_MODE must be dump or compare, not {other}"),
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut sink = Sink {
        dir,
        compare,
        rows: Vec::new(),
    };
    synthetic_cases(&mut sink);
    direct_cases(&mut sink);
    real_cases(&mut sink);
    sink.summary();
}
