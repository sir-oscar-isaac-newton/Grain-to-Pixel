//! Synthetic colour negatives with analytical ground truth.
//!
//! Tests of the pipeline stages must not depend on real film, so this module builds a negative
//! from a positive picture whose every parameter is known, and returns those parameters.
//!
//! # Forward model
//!
//! Per colour channel `c`, for a pixel inside the frame with positive value `P` in `[0, 1]`:
//!
//! ```text
//! D = black[c] + P * (white[c] - black[c])     density, black/white are densities
//! T = 10^(-D)                                   transmittance
//! I = base[c] * T                               scan value
//! ```
//!
//! Outside the frame (the rebate) there is no image, so `I = base[c]`. Dust specks multiply the
//! scan value of every channel by [`DUST_TRANSMISSION`] and lower the infrared plane from
//! [`IR_CLEAN`] to [`IR_DUST`]. Optional noise multiplies each sample by `1 + noise * u` with
//! `u` uniform in `[-1, 1]`.
//!
//! Inverting with the true `base`, `black` and `white` gives the positive back:
//! `P = (-log10(I / base) - black) / (white - black)` (see [`Truth::invert`]). That is the
//! contract the pipeline's film-base, inversion and detection stages are tested against.
//!
//! The generator is deterministic: the same seed and parameters give identical output on one
//! machine. Float `powf` may differ in the last digit between operating systems, so tests compare
//! within a tolerance and store no golden files.

use crate::error::{AppError, ErrorCode};
use crate::image::ImageF32;

/// Factor applied to the scan value of a pixel covered by dust.
pub const DUST_TRANSMISSION: f32 = 0.25;
/// Infrared plane value where there is no dust.
pub const IR_CLEAN: f32 = 1.0;
/// Infrared plane value under a dust speck.
pub const IR_DUST: f32 = 0.1;

const MAX_SIDE: u32 = 4096;
const MAX_DUST_SPECKS: u32 = 1000;
const MAX_NOISE: f32 = 0.5;
const MIN_DENSITY_SPAN: f32 = 1e-3;
const WEDGE_STEPS: f32 = 8.0;
const BLOB_COUNT: usize = 6;

/// What the positive picture looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pattern {
    /// Eight horizontal steps from 0 to 1, the same in every channel (a density wedge).
    Wedge,
    /// Red grows left to right, green top to bottom, blue is their mean.
    #[default]
    Gradient,
    /// A few seeded soft blobs on a dim background; different per seed.
    Blobs,
}

/// A rectangle in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Everything that shapes a synthetic negative.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub width: u32,
    pub height: u32,
    /// Film base (linear scan value of unexposed film) per channel, above 0.
    pub base: [f32; 3],
    /// Density of a positive 0 per channel.
    pub black: [f32; 3],
    /// Density of a positive 1 per channel; above `black`.
    pub white: [f32; 3],
    /// Rebate width in pixels at the left, top, right and bottom edge; the rest is the frame.
    pub rebate: [u32; 4],
    pub pattern: Pattern,
    pub dust_specks: u32,
    /// Also return an infrared plane (all clean when there is no dust).
    pub with_ir: bool,
    /// Relative noise amplitude, 0 to 0.5.
    pub noise: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            width: 96,
            height: 64,
            base: [0.80, 0.55, 0.35],
            black: [0.10, 0.12, 0.15],
            white: [2.10, 2.30, 2.50],
            rebate: [8; 4],
            pattern: Pattern::Gradient,
            dust_specks: 0,
            with_ir: false,
            noise: 0.0,
        }
    }
}

fn invalid(message: &str, details: String) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message).with_details(details)
}

impl Params {
    /// The frame rectangle implied by the size and the rebate.
    pub fn frame(&self) -> Result<PixelRect, AppError> {
        let [left, top, right, bottom] = self.rebate;
        let across = u64::from(left) + u64::from(right);
        let down = u64::from(top) + u64::from(bottom);
        if across >= u64::from(self.width) || down >= u64::from(self.height) {
            return Err(invalid(
                "The rebate leaves no room for a frame.",
                format!(
                    "{}x{} with rebate {:?}",
                    self.width, self.height, self.rebate
                ),
            ));
        }
        Ok(PixelRect {
            x: left,
            y: top,
            w: self.width - left - right,
            h: self.height - top - bottom,
        })
    }

    fn validate(&self) -> Result<(), AppError> {
        if !(1..=MAX_SIDE).contains(&self.width) || !(1..=MAX_SIDE).contains(&self.height) {
            return Err(invalid(
                "The image size is out of range.",
                format!("{}x{}", self.width, self.height),
            ));
        }
        for c in 0..3 {
            let (base, black, white) = (self.base[c], self.black[c], self.white[c]);
            if !(base.is_finite() && base > 0.0) {
                return Err(invalid("The film base must be above 0.", format!("{base}")));
            }
            let densities_ok = black.is_finite()
                && white.is_finite()
                && black >= 0.0
                && white >= black + MIN_DENSITY_SPAN;
            if !densities_ok {
                return Err(invalid(
                    "White must be a higher density than black.",
                    format!("channel {c}: black {black}, white {white}"),
                ));
            }
        }
        if !(0.0..=MAX_NOISE).contains(&self.noise) {
            return Err(invalid(
                "The noise amplitude is out of range.",
                format!("{}", self.noise),
            ));
        }
        if self.dust_specks > MAX_DUST_SPECKS {
            return Err(invalid(
                "Too many dust specks.",
                format!("{}", self.dust_specks),
            ));
        }
        self.frame().map(|_| ())
    }
}

/// The answers a stage under test should find.
#[derive(Debug, Clone, PartialEq)]
pub struct Truth {
    pub base: [f32; 3],
    /// Density of a positive 0.
    pub black: [f32; 3],
    /// Density of a positive 1.
    pub white: [f32; 3],
    pub frame: PixelRect,
    /// `width * height` flags, row-major: true where a dust speck is.
    pub dust_mask: Vec<bool>,
    /// The ideal positive without dust or noise; 0 outside the frame.
    pub expected_positive: ImageF32,
}

impl Truth {
    /// Inverts a scan with the true base and black/white points (the model run backwards).
    /// Returns values outside `[0, 1]` for pixels the model does not describe (dust, rebate).
    pub fn invert(&self, scan: &ImageF32) -> ImageF32 {
        let mut out = scan.clone();
        for pixel in out.data.chunks_exact_mut(usize::from(scan.channels)) {
            for (c, value) in pixel.iter_mut().enumerate().take(3) {
                let density = -(*value / self.base[c]).log10();
                *value = (density - self.black[c]) / (self.white[c] - self.black[c]);
            }
        }
        out
    }
}

/// A generated negative and its ground truth.
#[derive(Debug, Clone, PartialEq)]
pub struct Synthetic {
    /// The scan: 3 channels, linear.
    pub image: ImageF32,
    /// Infrared plane (1 channel) when requested.
    pub ir: Option<ImageF32>,
    pub truth: Truth,
}

/// SplitMix64: tiny, fast and fixed forever, so a seed always means the same picture.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f32 {
        // The top 24 bits fit an f32 mantissa exactly.
        (self.next_u64() >> 40) as f32 / 16_777_216.0
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

struct Blob {
    cx: f32,
    cy: f32,
    radius: f32,
    amplitude: [f32; 3],
}

fn make_blobs(rng: &mut Rng) -> Vec<Blob> {
    (0..BLOB_COUNT)
        .map(|_| Blob {
            cx: rng.unit(),
            cy: rng.unit(),
            radius: rng.range(0.08, 0.25),
            amplitude: [
                rng.range(0.0, 0.6),
                rng.range(0.0, 0.6),
                rng.range(0.0, 0.6),
            ],
        })
        .collect()
}

/// The positive value of channel `c` at frame coordinates `(u, v)`, both in `[0, 1]`.
fn positive(pattern: Pattern, blobs: &[Blob], u: f32, v: f32, c: usize) -> f32 {
    let value = match pattern {
        Pattern::Wedge => (u * WEDGE_STEPS).floor().min(WEDGE_STEPS - 1.0) / (WEDGE_STEPS - 1.0),
        Pattern::Gradient => [u, v, (u + v) / 2.0][c],
        Pattern::Blobs => {
            0.15 + blobs
                .iter()
                .map(|b| {
                    let d2 = (u - b.cx).powi(2) + (v - b.cy).powi(2);
                    b.amplitude[c] * (-d2 / (2.0 * b.radius * b.radius)).exp()
                })
                .sum::<f32>()
        }
    };
    value.clamp(0.0, 1.0)
}

fn fraction(index: u32, size: u32) -> f32 {
    if size <= 1 {
        0.0
    } else {
        index as f32 / (size - 1) as f32
    }
}

/// Dust specks: discs of 1 to 3 pixels radius whose centres are inside the frame.
fn make_dust_mask(rng: &mut Rng, params: &Params, frame: PixelRect) -> Vec<bool> {
    let mut mask = vec![false; params.width as usize * params.height as usize];
    for _ in 0..params.dust_specks {
        let cx = frame.x as f32 + rng.unit() * frame.w as f32;
        let cy = frame.y as f32 + rng.unit() * frame.h as f32;
        let radius = rng.range(1.0, 3.0);
        for y in frame.y..frame.y + frame.h {
            for x in frame.x..frame.x + frame.w {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= radius * radius {
                    mask[y as usize * params.width as usize + x as usize] = true;
                }
            }
        }
    }
    mask
}

/// Builds a negative from `seed` and `params` (see the module docs for the model).
pub fn negative(seed: u64, params: &Params) -> Result<Synthetic, AppError> {
    params.validate()?;
    let frame = params.frame()?;
    let mut rng = Rng(seed);
    // The draw order is part of the contract: blobs, then dust, then per-pixel noise.
    let blobs = if params.pattern == Pattern::Blobs {
        make_blobs(&mut rng)
    } else {
        Vec::new()
    };
    let dust_mask = make_dust_mask(&mut rng, params, frame);

    let mut image = ImageF32::zeros(params.width, params.height, 3)?;
    let mut expected = ImageF32::zeros(params.width, params.height, 3)?;
    let mut index = 0;
    for y in 0..params.height {
        for x in 0..params.width {
            let inside =
                x >= frame.x && x < frame.x + frame.w && y >= frame.y && y < frame.y + frame.h;
            let (u, v) = (
                fraction(x.wrapping_sub(frame.x), frame.w),
                fraction(y.wrapping_sub(frame.y), frame.h),
            );
            for c in 0..3 {
                let mut scan = params.base[c];
                if inside {
                    let p = positive(params.pattern, &blobs, u, v, c);
                    expected.data[index * 3 + c] = p;
                    let density = params.black[c] + p * (params.white[c] - params.black[c]);
                    scan = params.base[c] * 10f32.powf(-density);
                    if dust_mask[index] {
                        scan *= DUST_TRANSMISSION;
                    }
                }
                if params.noise > 0.0 {
                    scan *= 1.0 + params.noise * (2.0 * rng.unit() - 1.0);
                }
                image.data[index * 3 + c] = scan;
            }
            index += 1;
        }
    }

    let ir = if params.with_ir {
        let data = dust_mask
            .iter()
            .map(|&dusty| if dusty { IR_DUST } else { IR_CLEAN })
            .collect();
        Some(ImageF32::new(params.width, params.height, 1, data)?)
    } else {
        None
    };

    Ok(Synthetic {
        image,
        ir,
        truth: Truth {
            base: params.base,
            black: params.black,
            white: params.white,
            frame,
            dust_mask,
            expected_positive: expected,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(image: &ImageF32, x: u32, y: u32) -> &[f32] {
        let channels = usize::from(image.channels);
        let start = (y as usize * image.width as usize + x as usize) * channels;
        &image.data[start..start + channels]
    }

    fn inside(frame: PixelRect, x: u32, y: u32) -> bool {
        x >= frame.x && x < frame.x + frame.w && y >= frame.y && y < frame.y + frame.h
    }

    #[test]
    fn splitmix64_matches_its_reference_sequence() {
        let mut rng = Rng(0);
        assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(rng.next_u64(), 0x6E78_9E6A_A1B9_65F4);
    }

    #[test]
    fn the_same_seed_gives_identical_output_and_another_seed_differs() {
        let params = Params {
            pattern: Pattern::Blobs,
            dust_specks: 5,
            with_ir: true,
            noise: 0.02,
            ..Params::default()
        };
        let a = negative(7, &params).unwrap();
        let b = negative(7, &params).unwrap();
        assert_eq!(a, b);
        assert_ne!(a.image, negative(8, &params).unwrap().image);
    }

    #[test]
    fn the_frame_follows_the_rebate() {
        let params = Params {
            width: 50,
            height: 40,
            rebate: [4, 3, 6, 2],
            ..Params::default()
        };
        let s = negative(1, &params).unwrap();
        assert_eq!(
            s.truth.frame,
            PixelRect {
                x: 4,
                y: 3,
                w: 40,
                h: 35
            }
        );
    }

    #[test]
    fn the_rebate_is_exactly_the_film_base() {
        let s = negative(1, &Params::default()).unwrap();
        let frame = s.truth.frame;
        let mut rebate_pixels = 0;
        for y in 0..s.image.height {
            for x in 0..s.image.width {
                if !inside(frame, x, y) {
                    assert_eq!(pixel(&s.image, x, y), s.truth.base);
                    assert_eq!(pixel(&s.truth.expected_positive, x, y), [0.0; 3]);
                    rebate_pixels += 1;
                }
            }
        }
        assert!(rebate_pixels > 0);
    }

    #[test]
    fn inverting_with_the_truth_recovers_the_expected_positive() {
        for pattern in [Pattern::Wedge, Pattern::Gradient, Pattern::Blobs] {
            let s = negative(
                3,
                &Params {
                    pattern,
                    ..Params::default()
                },
            )
            .unwrap();
            let back = s.truth.invert(&s.image);
            for y in 0..s.image.height {
                for x in 0..s.image.width {
                    if inside(s.truth.frame, x, y) {
                        for (got, want) in
                            pixel(&back, x, y)
                                .iter()
                                .zip(pixel(&s.truth.expected_positive, x, y))
                        {
                            assert!(
                                (got - want).abs() < 1e-4,
                                "{pattern:?} at {x},{y}: {got} vs {want}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_expected_positive_stays_in_range_and_the_wedge_has_eight_steps() {
        let s = negative(
            1,
            &Params {
                pattern: Pattern::Wedge,
                ..Params::default()
            },
        )
        .unwrap();
        let frame = s.truth.frame;
        let mut levels: Vec<f32> = Vec::new();
        for x in frame.x..frame.x + frame.w {
            let p = pixel(&s.truth.expected_positive, x, frame.y)[0];
            assert!((0.0..=1.0).contains(&p));
            if !levels.iter().any(|l| (l - p).abs() < 1e-6) {
                levels.push(p);
            }
        }
        assert_eq!(levels.len(), 8);
        let blobs = negative(
            1,
            &Params {
                pattern: Pattern::Blobs,
                ..Params::default()
            },
        )
        .unwrap();
        assert!(blobs
            .truth
            .expected_positive
            .data
            .iter()
            .all(|p| (0.0..=1.0).contains(p)));
    }

    #[test]
    fn dust_darkens_the_scan_marks_the_ir_plane_and_stays_inside_the_frame() {
        let clean = negative(5, &Params::default()).unwrap();
        let dusty = negative(
            5,
            &Params {
                dust_specks: 6,
                with_ir: true,
                ..Params::default()
            },
        )
        .unwrap();
        let ir = dusty.ir.as_ref().expect("an IR plane was requested");
        let mut specks = 0;
        for y in 0..dusty.image.height {
            for x in 0..dusty.image.width {
                let flagged =
                    dusty.truth.dust_mask[y as usize * dusty.image.width as usize + x as usize];
                if flagged {
                    specks += 1;
                    assert!(inside(dusty.truth.frame, x, y));
                    assert_eq!(pixel(ir, x, y)[0], IR_DUST);
                    for (d, c) in pixel(&dusty.image, x, y)
                        .iter()
                        .zip(pixel(&clean.image, x, y))
                    {
                        assert!((d - c * DUST_TRANSMISSION).abs() < 1e-6);
                    }
                } else {
                    assert_eq!(pixel(ir, x, y)[0], IR_CLEAN);
                    assert_eq!(pixel(&dusty.image, x, y), pixel(&clean.image, x, y));
                }
            }
        }
        assert!(specks > 0);
        assert_eq!(
            dusty.truth.expected_positive, clean.truth.expected_positive,
            "truth ignores dust"
        );
    }

    #[test]
    fn the_ir_plane_is_only_returned_when_requested() {
        assert!(negative(1, &Params::default()).unwrap().ir.is_none());
        let ir = negative(
            1,
            &Params {
                with_ir: true,
                ..Params::default()
            },
        )
        .unwrap()
        .ir
        .unwrap();
        assert!(ir.data.iter().all(|&v| v == IR_CLEAN));
    }

    #[test]
    fn noise_perturbs_the_scan_within_its_amplitude() {
        let clean = negative(2, &Params::default()).unwrap();
        let noisy = negative(
            2,
            &Params {
                noise: 0.05,
                ..Params::default()
            },
        )
        .unwrap();
        assert_ne!(clean.image, noisy.image);
        for (n, c) in noisy.image.data.iter().zip(&clean.image.data) {
            assert!((n / c - 1.0).abs() <= 0.05 + 1e-6);
        }
    }

    #[test]
    fn bad_parameters_are_invalid_input() {
        let bad = [
            Params {
                width: 0,
                ..Params::default()
            },
            Params {
                height: MAX_SIDE + 1,
                ..Params::default()
            },
            Params {
                rebate: [60, 0, 40, 0],
                ..Params::default()
            },
            Params {
                base: [0.8, 0.0, 0.3],
                ..Params::default()
            },
            Params {
                white: [2.0, 0.05, 2.0],
                ..Params::default()
            },
            Params {
                black: [f32::NAN, 0.1, 0.1],
                ..Params::default()
            },
            Params {
                noise: 0.9,
                ..Params::default()
            },
            Params {
                dust_specks: MAX_DUST_SPECKS + 1,
                ..Params::default()
            },
        ];
        for params in bad {
            let err = negative(1, &params).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{params:?}");
        }
    }
}
