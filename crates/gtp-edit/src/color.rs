//! Colour stage (docs/specification.md section 4.2 step 6), applied in place in this order:
//!
//! 1. **Channel gains** from the three colour axes, temperature and tint, in stops (powers of 2).
//!    Each control raises its own channel(s) and lowers the others so that the gains always
//!    multiply to 1: the overall brightness (geometric mean of the channels) stays put.
//!    - `red_cyan = a` (a = slider / 100): R by `a * AXIS_STOPS`, G and B by `-a * AXIS_STOPS / 2`;
//!      likewise `green_magenta` (G) and `blue_yellow` (B). Positive adds red, green or blue;
//!      negative adds cyan, magenta or yellow, as in darkroom filtration.
//!    - `temperature = t`: R by `t * TEMPERATURE_STOPS / 2`, B by `-t * TEMPERATURE_STOPS / 2`
//!      (positive is warmer).
//!    - `tint = t`: G by `-t * TINT_STOPS * 2 / 3`, R and B by `t * TINT_STOPS / 3` (positive is
//!      magenta, negative green).
//! 2. **Black and white** (`Mono::enabled`): every pixel becomes `mix . rgb` in all three channels;
//!    saturation and vibrance are skipped.
//! 3. **Saturation** around the Rec.2020 luma: `c' = Y + (c - Y) * saturation`.
//! 4. **Vibrance**: the same, with the factor `1 + v * (1 - s)` where `s = (max - min) / max` is the
//!    pixel's own saturation, so muted colours move more than vivid ones.
//!
//! Samples are first limited to `[0, MAX_SAMPLE]` (not finite becomes 0), so no setting can produce
//! infinity; results below 0 (out of gamut after a strong saturation) are set to 0.

use gtp_core::recipe::{Color, Mono};
use gtp_core::{AppError, ImageF32};
use rayon::prelude::*;

use crate::{require_rgb, saturate, MAX_SAMPLE};

/// Stops on the main channel at a colour axis of +-100.
pub const AXIS_STOPS: f32 = 1.0;
/// Stops between red and blue at a temperature of +-100.
pub const TEMPERATURE_STOPS: f32 = 1.0;
/// Stops between green and red/blue at a tint of +-100.
pub const TINT_STOPS: f32 = 1.0;

/// The per-channel gains of the colour axes, temperature and tint. They multiply to 1.
pub fn channel_gains(color: &Color) -> [f32; 3] {
    let axis = |slider: f32| slider / 100.0 * AXIS_STOPS;
    let (rc, gm, by) = (
        axis(color.red_cyan),
        axis(color.green_magenta),
        axis(color.blue_yellow),
    );
    let temperature = color.temperature / 100.0 * TEMPERATURE_STOPS / 2.0;
    let tint = color.tint / 100.0 * TINT_STOPS / 3.0;
    let stops = [
        rc - gm / 2.0 - by / 2.0 + temperature + tint,
        gm - rc / 2.0 - by / 2.0 - 2.0 * tint,
        by - rc / 2.0 - gm / 2.0 - temperature + tint,
    ];
    stops.map(f32::exp2)
}

/// Applies the colour stage to a 3-channel image in place (see the module docs for the order).
pub fn apply_color(image: &mut ImageF32, color: &Color, mono: &Mono) -> Result<(), AppError> {
    require_rgb(image, "colour")?;
    let gains = channel_gains(color);
    let use_gains = gains.iter().any(|g| *g != 1.0);
    let saturation = color.saturation;
    let vibrance = color.vibrance / 100.0;

    let (pixels, _) = image.data.as_chunks_mut::<3>();
    pixels.par_iter_mut().for_each(|pixel| {
        for c in pixel.iter_mut() {
            *c = if c.is_finite() {
                c.clamp(0.0, MAX_SAMPLE)
            } else {
                0.0
            };
        }
        if use_gains {
            for (c, gain) in pixel.iter_mut().zip(gains) {
                *c *= gain;
            }
        }
        if mono.enabled {
            let y = mono.mix[0] * pixel[0] + mono.mix[1] * pixel[1] + mono.mix[2] * pixel[2];
            *pixel = [y.max(0.0); 3];
            return;
        }
        if saturation != 1.0 {
            saturate(pixel, saturation);
        }
        if vibrance != 0.0 {
            let max = pixel[0].max(pixel[1]).max(pixel[2]);
            let min = pixel[0].min(pixel[1]).min(pixel[2]);
            let own = if max > 0.0 { (max - min) / max } else { 0.0 };
            saturate(pixel, 1.0 + vibrance * (1.0 - own));
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use gtp_core::recipe::REC2020_LUMA;
    use gtp_core::ErrorCode;
    use proptest::prelude::*;

    use super::*;
    use crate::luma;

    fn image(pixels: &[[f32; 3]]) -> ImageF32 {
        ImageF32::new(pixels.len() as u32, 1, 3, pixels.concat()).unwrap()
    }

    fn pixel(img: &ImageF32, i: usize) -> [f32; 3] {
        [img.data[i * 3], img.data[i * 3 + 1], img.data[i * 3 + 2]]
    }

    fn saturation_of(p: [f32; 3]) -> f32 {
        let max = p[0].max(p[1]).max(p[2]);
        (max - p[0].min(p[1]).min(p[2])) / max
    }

    #[test]
    fn the_default_recipe_changes_nothing() {
        let data = [[0.1, 0.5, 0.9], [0.0, 0.0, 0.0], [2.5, 0.3, 0.7]];
        let mut img = image(&data);
        apply_color(&mut img, &Color::default(), &Mono::default()).unwrap();
        assert_eq!(img, image(&data));
    }

    #[test]
    fn each_axis_raises_its_channel_and_the_gains_multiply_to_one() {
        let red = channel_gains(&Color {
            red_cyan: 100.0,
            ..Color::default()
        });
        assert!((red[0] - 2.0).abs() < 1e-6);
        assert!((red[1] - 2f32.powf(-0.5)).abs() < 1e-6 && (red[2] - red[1]).abs() < 1e-7);
        let cyan = channel_gains(&Color {
            red_cyan: -100.0,
            ..Color::default()
        });
        assert!(
            cyan[0] < 1.0 && cyan[1] > 1.0 && cyan[2] > 1.0,
            "cyan = less red"
        );
        let green = channel_gains(&Color {
            green_magenta: 50.0,
            ..Color::default()
        });
        assert!(green[1] > 1.0 && green[0] < 1.0 && green[2] < 1.0);
        let yellow = channel_gains(&Color {
            blue_yellow: -50.0,
            ..Color::default()
        });
        assert!(
            yellow[2] < 1.0 && yellow[0] > 1.0 && yellow[1] > 1.0,
            "yellow = less blue"
        );
    }

    #[test]
    fn temperature_warms_and_tint_moves_between_green_and_magenta() {
        let warm = channel_gains(&Color {
            temperature: 60.0,
            ..Color::default()
        });
        assert!(warm[0] > 1.0 && warm[2] < 1.0 && (warm[1] - 1.0).abs() < 1e-6);
        let magenta = channel_gains(&Color {
            tint: 60.0,
            ..Color::default()
        });
        assert!(magenta[1] < 1.0 && magenta[0] > 1.0 && magenta[2] > 1.0);
        let green = channel_gains(&Color {
            tint: -60.0,
            ..Color::default()
        });
        assert!(green[1] > 1.0);
    }

    #[test]
    fn saturation_zero_is_grey_and_saturation_keeps_luma() {
        let colour = [0.4, 0.3, 0.2];
        let mut grey = image(&[colour]);
        apply_color(
            &mut grey,
            &Color {
                saturation: 0.0,
                ..Color::default()
            },
            &Mono::default(),
        )
        .unwrap();
        let g = pixel(&grey, 0);
        assert!((g[0] - g[1]).abs() < 1e-6 && (g[1] - g[2]).abs() < 1e-6);
        assert!((g[0] - luma(colour)).abs() < 1e-6);

        let mut more = image(&[colour]);
        apply_color(
            &mut more,
            &Color {
                saturation: 1.5,
                ..Color::default()
            },
            &Mono::default(),
        )
        .unwrap();
        assert!((luma(pixel(&more, 0)) - luma(colour)).abs() < 1e-6);
        assert!(saturation_of(pixel(&more, 0)) > saturation_of(colour));
    }

    #[test]
    fn vibrance_moves_muted_colours_more_than_vivid_ones() {
        let (muted, vivid) = ([0.45, 0.4, 0.35], [0.9, 0.2, 0.1]);
        let mut img = image(&[muted, vivid]);
        apply_color(
            &mut img,
            &Color {
                vibrance: 60.0,
                ..Color::default()
            },
            &Mono::default(),
        )
        .unwrap();
        let muted_gain = saturation_of(pixel(&img, 0)) / saturation_of(muted);
        let vivid_gain = saturation_of(pixel(&img, 1)) / saturation_of(vivid);
        assert!(muted_gain > vivid_gain, "{muted_gain} vs {vivid_gain}");
        assert!(muted_gain > 1.0);
    }

    #[test]
    fn black_and_white_uses_the_mix_and_ignores_saturation() {
        let colour = [0.6, 0.3, 0.1];
        let mut img = image(&[colour]);
        let mono = Mono {
            enabled: true,
            mix: [0.5, 0.5, 0.0],
        };
        apply_color(
            &mut img,
            &Color {
                saturation: 2.0,
                vibrance: 100.0,
                ..Color::default()
            },
            &mono,
        )
        .unwrap();
        for v in pixel(&img, 0) {
            assert!((v - 0.45).abs() < 1e-6, "{v}");
        }

        let mut luma_mix = image(&[colour]);
        apply_color(
            &mut luma_mix,
            &Color::default(),
            &Mono {
                enabled: true,
                ..Mono::default()
            },
        )
        .unwrap();
        let expected = REC2020_LUMA[0] * 0.6 + REC2020_LUMA[1] * 0.3 + REC2020_LUMA[2] * 0.1;
        assert!((pixel(&luma_mix, 0)[0] - expected).abs() < 1e-6);
    }

    #[test]
    fn a_red_filter_mix_darkens_blue_more_than_red() {
        let red_filter = Mono {
            enabled: true,
            mix: [1.0, 0.0, 0.0],
        };
        let mut img = image(&[[0.8, 0.2, 0.1], [0.1, 0.2, 0.8]]);
        apply_color(&mut img, &Color::default(), &red_filter).unwrap();
        assert!(
            pixel(&img, 0)[0] > pixel(&img, 1)[0],
            "a red object stays light, blue sky goes dark"
        );
    }

    #[test]
    fn a_non_rgb_image_is_rejected() {
        let mut gray = ImageF32::zeros(2, 2, 1).unwrap();
        let err = apply_color(&mut gray, &Color::default(), &Mono::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    proptest! {
        #[test]
        fn gains_always_multiply_to_one(
            rc in -100f32..=100.0, gm in -100f32..=100.0, by in -100f32..=100.0,
            t in -100f32..=100.0, tint in -100f32..=100.0,
        ) {
            let g = channel_gains(&Color { red_cyan: rc, green_magenta: gm, blue_yellow: by, temperature: t, tint, ..Color::default() });
            prop_assert!((g[0] * g[1] * g[2] - 1.0).abs() < 1e-4);
        }

        #[test]
        fn the_output_is_finite_and_not_negative(
            data in proptest::collection::vec(proptest::num::f32::ANY, 3..30),
            saturation in 0f32..=2.0, vibrance in -100f32..=100.0, rc in -100f32..=100.0, enabled: bool,
        ) {
            let mut data = data;
            data.truncate(data.len() / 3 * 3);
            let mut img = ImageF32::new((data.len() / 3) as u32, 1, 3, data).unwrap();
            let color = Color { saturation, vibrance, red_cyan: rc, ..Color::default() };
            apply_color(&mut img, &color, &Mono { enabled, ..Mono::default() }).unwrap();
            prop_assert!(img.data.iter().all(|v| v.is_finite() && *v >= 0.0), "{:?}", img.data);
        }
    }
}
