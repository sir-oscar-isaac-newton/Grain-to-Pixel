//! Tone stage (docs/specification.md section 4.2 step 7), applied in place in this order:
//!
//! 1. **Exposure**: `Y * 2^exposure_ev`.
//! 2. **Contrast**, a power curve around [`MID_GREY`]: `MID_GREY * (Y / MID_GREY)^k` with
//!    `k = 2^(contrast / 100)`; monotone, and mid-grey stays where it is.
//! 3. **Highlights and shadows**: a gain of up to [`REGION_STOPS`] stops, weighted by where the
//!    pixel sits relative to mid-grey (`x = log2(Y / MID_GREY)`): highlights fade in from mid-grey
//!    to [`HIGHLIGHT_RANGE_STOPS`] above it, shadows from mid-grey to [`SHADOW_RANGE_STOPS`] below.
//!    Mid-grey itself is never touched, and the weights rise slowly enough to keep the result
//!    monotone.
//! 4. **Whites and blacks** move the white and black points: `(Y - B) / (W - B)` with
//!    `W = 1 - WHITES_RANGE * whites / 100` and `B = -BLACKS_RANGE * blacks / 100`. Positive whites
//!    brighten the top, positive blacks lift the bottom, negative blacks crush it.
//!
//! Steps 1 to 4 act on the Rec.2020 luma `Y` and scale the pixel by `Y' / Y`, so hue and saturation
//! stay as they are. Then:
//!
//! 5. **Dehaze**, global: positive values remove a uniform veil (film fog, scanner flare) estimated
//!    as the [`DEHAZE_VEIL_PERCENTILE`] percentile of each pixel's darkest channel, `c' = (c - d*v) /
//!    (1 - d*v)`, which keeps 1.0 fixed, and lift saturation by up to [`DEHAZE_SATURATION`]; negative
//!    values add a veil of up to [`DEHAZE_ADD_VEIL`] and lower saturation.
//! 6. **Curves**: master on every channel, then the R, G and B curves.
//!
//! Samples are limited to `[0, MAX_SAMPLE]` first (not finite becomes 0); results never go below 0.

use gtp_core::recipe::Tone;
use gtp_core::{AppError, ImageF32};

use crate::curve::Curve;
use crate::{luma, require_rgb, saturate, MAX_SAMPLE};

/// Photographic mid-grey in linear light: the contrast pivot and the highlight/shadow divide.
pub const MID_GREY: f32 = 0.18;
/// Stops of gain at highlights or shadows of +-100 where the weight is full.
pub const REGION_STOPS: f32 = 1.0;
/// Highlights reach full weight this many stops above mid-grey.
pub const HIGHLIGHT_RANGE_STOPS: f32 = 2.5;
/// Shadows reach full weight this many stops below mid-grey.
pub const SHADOW_RANGE_STOPS: f32 = 4.0;
/// How far whites of +-100 move the white point.
pub const WHITES_RANGE: f32 = 0.25;
/// How far blacks of +-100 move the black point.
pub const BLACKS_RANGE: f32 = 0.05;
/// Percentile of the darkest channel taken as the veil.
pub const DEHAZE_VEIL_PERCENTILE: f32 = 0.5;
/// Veil added at a dehaze of -100.
pub const DEHAZE_ADD_VEIL: f32 = 0.25;
/// Saturation change at a dehaze of +-100.
pub const DEHAZE_SATURATION: f32 = 0.25;

/// Below this luma a pixel is treated as black and set to the mapped grey.
const LUMA_EPSILON: f32 = 1e-6;
/// The veil estimate looks at about this many pixels at most.
const MAX_VEIL_SAMPLES: u64 = 1_000_000;
/// The largest veil that is removed (keeps the division well away from zero).
const MAX_VEIL: f32 = 0.9;

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Steps 1 to 4 as a function of luma.
struct ToneMap {
    exposure: f32,
    contrast: f32,
    highlights: f32,
    shadows: f32,
    white: f32,
    black: f32,
}

impl ToneMap {
    fn new(tone: &Tone) -> Self {
        Self {
            exposure: tone.exposure_ev.exp2(),
            contrast: (tone.contrast / 100.0).exp2(),
            highlights: tone.highlights / 100.0 * REGION_STOPS,
            shadows: tone.shadows / 100.0 * REGION_STOPS,
            white: 1.0 - WHITES_RANGE * tone.whites / 100.0,
            black: -BLACKS_RANGE * tone.blacks / 100.0,
        }
    }

    fn is_identity(&self) -> bool {
        self.exposure == 1.0
            && self.contrast == 1.0
            && self.highlights == 0.0
            && self.shadows == 0.0
            && self.white == 1.0
            && self.black == 0.0
    }

    fn map(&self, luma: f32) -> f32 {
        let mut y = luma * self.exposure;
        if self.contrast != 1.0 && y > 0.0 {
            y = MID_GREY * (y / MID_GREY).powf(self.contrast);
        }
        if (self.highlights != 0.0 || self.shadows != 0.0) && y > 0.0 {
            let x = (y / MID_GREY).log2();
            let stops = self.highlights * smoothstep(0.0, HIGHLIGHT_RANGE_STOPS, x)
                + self.shadows * (1.0 - smoothstep(-SHADOW_RANGE_STOPS, 0.0, x));
            y *= stops.exp2();
        }
        if self.white != 1.0 || self.black != 0.0 {
            y = (y - self.black) / (self.white - self.black);
        }
        y.clamp(0.0, MAX_SAMPLE)
    }
}

/// The value at `percent` of `values` (reorders them); `None` if empty.
fn percentile(values: &mut [f32], percent: f32) -> Option<f32> {
    let last = values.len().checked_sub(1)?;
    let index = ((percent / 100.0) * last as f32).round() as usize;
    let (_, value, _) = values.select_nth_unstable_by(index.min(last), f32::total_cmp);
    Some(*value)
}

fn dehaze(image: &mut ImageF32, amount: f32) {
    let (pixels, _) = image.data.as_chunks_mut::<3>();
    if amount > 0.0 {
        // The estimate needs the whole image first; it is a separate, read-only pass.
        let veil = amount * estimate_veil_from(pixels);
        for pixel in pixels {
            if veil > 0.0 {
                for c in pixel.iter_mut() {
                    *c = ((*c - veil) / (1.0 - veil)).max(0.0);
                }
            }
            saturate(pixel, 1.0 + DEHAZE_SATURATION * amount);
        }
    } else {
        let veil = -amount * DEHAZE_ADD_VEIL;
        for pixel in pixels {
            for c in pixel.iter_mut() {
                *c = *c * (1.0 - veil) + veil;
            }
            saturate(pixel, 1.0 + DEHAZE_SATURATION * amount);
        }
    }
}

/// The veil: a low percentile of the darkest channel of (up to about a million) sampled pixels.
fn estimate_veil_from(pixels: &[[f32; 3]]) -> f32 {
    let step = ((pixels.len() as f64 / MAX_VEIL_SAMPLES as f64).ceil() as usize).max(1);
    let mut darkest: Vec<f32> = pixels
        .iter()
        .step_by(step)
        .map(|p| p[0].min(p[1]).min(p[2]))
        .collect();
    percentile(&mut darkest, DEHAZE_VEIL_PERCENTILE)
        .unwrap_or(0.0)
        .clamp(0.0, MAX_VEIL)
}

/// Applies the tone stage to a 3-channel image in place (see the module docs for the order).
/// Fails only if a curve has invalid points.
pub fn apply_tone(image: &mut ImageF32, tone: &Tone) -> Result<(), AppError> {
    require_rgb(image, "tone")?;
    let master = Curve::new(&tone.curve.master)?;
    let channel_curves = [
        Curve::new(&tone.curve.r)?,
        Curve::new(&tone.curve.g)?,
        Curve::new(&tone.curve.b)?,
    ];
    let map = ToneMap::new(tone);

    let (pixels, _) = image.data.as_chunks_mut::<3>();
    for pixel in pixels.iter_mut() {
        for c in pixel.iter_mut() {
            *c = if c.is_finite() {
                c.clamp(0.0, MAX_SAMPLE)
            } else {
                0.0
            };
        }
        if map.is_identity() {
            continue;
        }
        let y = luma(*pixel);
        let mapped = map.map(y);
        if y > LUMA_EPSILON {
            let ratio = mapped / y;
            for c in pixel.iter_mut() {
                *c = (*c * ratio).clamp(0.0, MAX_SAMPLE);
            }
        } else {
            *pixel = [mapped; 3];
        }
    }

    if tone.dehaze != 0.0 {
        dehaze(image, tone.dehaze / 100.0);
    }

    if !master.is_identity() || channel_curves.iter().any(|c| !c.is_identity()) {
        let (pixels, _) = image.data.as_chunks_mut::<3>();
        for pixel in pixels {
            for (c, curve) in pixel.iter_mut().zip(&channel_curves) {
                *c = curve.eval(master.eval(*c));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use gtp_core::recipe::Curves;
    use gtp_core::ErrorCode;
    use proptest::prelude::*;

    use super::*;

    fn grey(values: &[f32]) -> ImageF32 {
        let data = values.iter().flat_map(|v| [*v; 3]).collect();
        ImageF32::new(values.len() as u32, 1, 3, data).unwrap()
    }

    fn values(img: &ImageF32) -> Vec<f32> {
        img.data.iter().step_by(3).copied().collect()
    }

    fn toned(input: &[f32], tone: &Tone) -> Vec<f32> {
        let mut img = grey(input);
        apply_tone(&mut img, tone).unwrap();
        values(&img)
    }

    #[test]
    fn the_default_recipe_changes_nothing() {
        let data = vec![0.0, 0.01, 0.18, 0.5, 1.0, 3.0, 0.2, 0.4, 0.6];
        let mut img = ImageF32::new(3, 1, 3, data.clone()).unwrap();
        apply_tone(&mut img, &Tone::default()).unwrap();
        for (a, b) in img.data.iter().zip(&data) {
            assert!((a - b).abs() < 1e-6, "{a} vs {b}");
        }
    }

    #[test]
    fn one_stop_of_exposure_doubles_linear_values() {
        let out = toned(
            &[0.1, 0.25, 2.0],
            &Tone {
                exposure_ev: 1.0,
                ..Tone::default()
            },
        );
        for (got, want) in out.iter().zip([0.2, 0.5, 4.0]) {
            assert!((got - want).abs() < 1e-5, "{got} vs {want}");
        }
        let down = toned(
            &[0.4],
            &Tone {
                exposure_ev: -1.0,
                ..Tone::default()
            },
        );
        assert!((down[0] - 0.2).abs() < 1e-6);
    }

    #[test]
    fn contrast_pivots_on_mid_grey() {
        assert_eq!(toned(&[0.05, 0.6], &Tone::default()), vec![0.05, 0.6]);
        for contrast in [-100.0, -40.0, 50.0, 100.0] {
            let out = toned(
                &[MID_GREY],
                &Tone {
                    contrast,
                    ..Tone::default()
                },
            );
            assert!((out[0] - MID_GREY).abs() < 1e-6, "{contrast}");
        }
        let more = toned(
            &[0.05, 0.6],
            &Tone {
                contrast: 60.0,
                ..Tone::default()
            },
        );
        assert!(
            more[0] < 0.05 && more[1] > 0.6,
            "more contrast spreads values apart"
        );
        let less = toned(
            &[0.05, 0.6],
            &Tone {
                contrast: -60.0,
                ..Tone::default()
            },
        );
        assert!(less[0] > 0.05 && less[1] < 0.6);
    }

    #[test]
    fn highlights_and_shadows_act_on_their_own_region_and_leave_mid_grey() {
        let input = [0.005, MID_GREY, 1.0];
        let lifted = toned(
            &input,
            &Tone {
                highlights: 100.0,
                ..Tone::default()
            },
        );
        assert!((lifted[0] - 0.005).abs() < 1e-6, "shadows untouched");
        assert!((lifted[1] - MID_GREY).abs() < 1e-6, "mid-grey untouched");
        assert!(
            lifted[2] > 1.9,
            "a full-weight highlight gains about a stop: {}",
            lifted[2]
        );

        let opened = toned(
            &input,
            &Tone {
                shadows: 100.0,
                ..Tone::default()
            },
        );
        assert!(
            (opened[0] - 0.01).abs() < 1e-5,
            "a deep shadow gains a stop: {}",
            opened[0]
        );
        assert!((opened[1] - MID_GREY).abs() < 1e-6);
        assert!((opened[2] - 1.0).abs() < 1e-6, "highlights untouched");
    }

    #[test]
    fn whites_and_blacks_move_the_end_points() {
        let whites = toned(
            &[0.75, 0.0],
            &Tone {
                whites: 100.0,
                ..Tone::default()
            },
        );
        assert!((whites[0] - 1.0).abs() < 1e-6 && whites[1] == 0.0);
        let lifted = toned(
            &[0.0],
            &Tone {
                blacks: 100.0,
                ..Tone::default()
            },
        );
        assert!((lifted[0] - 0.05 / 1.05).abs() < 1e-6);
        let crushed = toned(
            &[0.05, 0.02],
            &Tone {
                blacks: -100.0,
                ..Tone::default()
            },
        );
        assert_eq!(crushed, vec![0.0, 0.0]);
    }

    #[test]
    fn tone_keeps_the_hue_of_a_colour() {
        let mut img = ImageF32::new(1, 1, 3, vec![0.3, 0.15, 0.05]).unwrap();
        apply_tone(
            &mut img,
            &Tone {
                exposure_ev: 0.7,
                contrast: 30.0,
                shadows: 40.0,
                ..Tone::default()
            },
        )
        .unwrap();
        let ratio = |p: &[f32]| (p[0] / p[1], p[1] / p[2]);
        let (a, b) = (ratio(&img.data), ratio(&[0.3, 0.15, 0.05]));
        assert!((a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4);
    }

    #[test]
    fn dehaze_removes_a_uniform_veil() {
        let clean: Vec<f32> = (0..1000).map(|i| i as f32 / 999.0).collect();
        let veiled: Vec<f32> = clean.iter().map(|j| 0.8 * j + 0.2).collect();
        let out = toned(
            &veiled,
            &Tone {
                dehaze: 100.0,
                ..Tone::default()
            },
        );
        for (got, want) in out.iter().zip(&clean) {
            assert!((got - want).abs() < 0.01, "{got} vs {want}");
        }
        let half = toned(
            &veiled,
            &Tone {
                dehaze: 50.0,
                ..Tone::default()
            },
        );
        assert!(
            half[0] > out[0] && half[0] < veiled[0],
            "half the dehaze removes part of the veil"
        );
    }

    #[test]
    fn negative_dehaze_adds_a_veil_and_dehaze_lifts_saturation() {
        let out = toned(
            &[0.0, 1.0],
            &Tone {
                dehaze: -100.0,
                ..Tone::default()
            },
        );
        assert!((out[0] - DEHAZE_ADD_VEIL).abs() < 1e-6 && (out[1] - 1.0).abs() < 1e-6);

        let colour = [0.5, 0.3, 0.25];
        let mut img = ImageF32::new(1, 1, 3, colour.to_vec()).unwrap();
        apply_tone(
            &mut img,
            &Tone {
                dehaze: 100.0,
                ..Tone::default()
            },
        )
        .unwrap();
        let spread = |p: &[f32]| p[0] - p[2];
        assert!(
            spread(&img.data) > spread(&colour) * 0.99,
            "colour is not washed out"
        );
    }

    #[test]
    fn curves_apply_master_then_each_channel() {
        let tone = Tone {
            curve: Curves {
                master: vec![[0.0, 0.0], [1.0, 1.0]],
                r: vec![[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]],
                g: Vec::new(),
                b: Vec::new(),
            },
            ..Tone::default()
        };
        let mut img = ImageF32::new(1, 1, 3, vec![0.5, 0.5, 0.5]).unwrap();
        apply_tone(&mut img, &tone).unwrap();
        assert!((img.data[0] - 0.75).abs() < 1e-3);
        assert!((img.data[1] - 0.5).abs() < 1e-6 && (img.data[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn invalid_curves_and_non_rgb_images_are_errors() {
        let bad = Tone {
            curve: Curves {
                master: vec![[0.5, 0.5]],
                ..Curves::default()
            },
            ..Tone::default()
        };
        assert_eq!(
            apply_tone(&mut grey(&[0.5]), &bad).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let mut gray = ImageF32::zeros(2, 2, 1).unwrap();
        assert_eq!(
            apply_tone(&mut gray, &Tone::default()).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn the_buffer_is_edited_in_place() {
        let mut img = ImageF32::new(256, 256, 3, vec![0.3; 256 * 256 * 3]).unwrap();
        let (pointer, capacity) = (img.data.as_ptr(), img.data.capacity());
        let tone = Tone {
            exposure_ev: 0.5,
            contrast: 20.0,
            highlights: -30.0,
            dehaze: 40.0,
            ..Tone::default()
        };
        apply_tone(&mut img, &tone).unwrap();
        assert_eq!(
            (img.data.as_ptr(), img.data.capacity()),
            (pointer, capacity)
        );
    }

    /// 21 megapixels (5616 x 3744) through the colour and tone stages; run with
    /// `cargo test -p gtp-edit --release -- --ignored --nocapture` to see the time.
    #[test]
    #[ignore = "timing run, not a check"]
    fn time_a_21_megapixel_image() {
        let (w, h) = (5616u32, 3744u32);
        let data: Vec<f32> = (0..w as usize * h as usize * 3)
            .map(|i| (i % 1000) as f32 / 1000.0)
            .collect();
        let mut img = ImageF32::new(w, h, 3, data).unwrap();
        let color = gtp_core::recipe::Color {
            red_cyan: 10.0,
            saturation: 1.1,
            vibrance: 20.0,
            ..Default::default()
        };
        let tone = Tone {
            exposure_ev: 0.3,
            contrast: 20.0,
            highlights: -20.0,
            shadows: 20.0,
            dehaze: 20.0,
            ..Tone::default()
        };
        let start = std::time::Instant::now();
        crate::apply_color(&mut img, &color, &gtp_core::recipe::Mono::default()).unwrap();
        apply_tone(&mut img, &tone).unwrap();
        eprintln!("21 MP colour + tone: {:?}", start.elapsed());
    }

    proptest! {
        /// For grey input, brighter in means brighter (or equal) out, for any slider settings.
        #[test]
        fn the_tone_map_is_monotone(
            exposure_ev in -5f32..=5.0,
            contrast in -100f32..=100.0,
            highlights in -100f32..=100.0,
            shadows in -100f32..=100.0,
            whites in -100f32..=100.0,
            blacks in -100f32..=100.0,
        ) {
            let input: Vec<f32> = (0..400).map(|i| (i as f32 / 100.0).powi(2) / 4.0).collect();
            let tone = Tone { exposure_ev, contrast, highlights, shadows, whites, blacks, ..Tone::default() };
            let out = toned(&input, &tone);
            for pair in out.windows(2) {
                prop_assert!(pair[1] >= pair[0] - 1e-6 * pair[0].abs().max(1.0), "{} then {}", pair[0], pair[1]);
            }
        }

        #[test]
        fn the_output_is_finite_and_not_negative(
            data in proptest::collection::vec(proptest::num::f32::ANY, 3..30),
            exposure_ev in -5f32..=5.0, contrast in -100f32..=100.0, dehaze in -100f32..=100.0,
        ) {
            let mut data = data;
            data.truncate(data.len() / 3 * 3);
            let mut img = ImageF32::new((data.len() / 3) as u32, 1, 3, data).unwrap();
            apply_tone(&mut img, &Tone { exposure_ev, contrast, dehaze, ..Tone::default() }).unwrap();
            prop_assert!(img.data.iter().all(|v| v.is_finite() && *v >= 0.0), "{:?}", img.data);
        }
    }
}
