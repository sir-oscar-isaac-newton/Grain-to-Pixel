//! The values of a look (P-19, decisions/0008-film-log-look.md).
//!
//! **Film log** is a flat, wide-latitude starting point for editing. The inversion's output `P` is
//! linear in density between the black and white points, and density is the logarithm of
//! exposure (times the film's gamma), so scene light follows from `P`:
//!
//! ```text
//! stops across the range  R = (white - black) / (FILM_GAMMA * log10 2)     per frame, mean of channels
//! scene light             s = MID_GREY_SCENE * 2^((P - 0.5) * R)           the middle of the range is grey
//! code                    c = gen5_encode(s)                               Blackmagic Film Gen 5
//! ```
//!
//! The code values are meant to be shown as they are, the way log footage looks on a monitor with
//! no LUT, so the recipe's master curve (which works in linear light before the display encoding)
//! is `srgb_decode(c)`: after the export or display encoding the picture shows the code values.
//! The curve is 16 points, spaced evenly in `P`, and follows the exact function to within
//! [`CURVE_TOLERANCE`] in code value.

use gtp_core::recipe::{CurvePoint, Invert, CURVE_POINTS_MAX};
use gtp_core::Look;

/// Blackmagic Film Generation 5 transfer function constants, from Blackmagic Design's
/// "Blackmagic Generation 5 Color Science" (2021) as implemented by the `colour` library; the
/// whitepaper itself was not read, so the tests pin the values of the published function:
/// 18 % grey encodes to 0.38356 and the two branches meet at [`GEN5_LOG_CUT`].
pub const GEN5_A: f64 = 0.086_928_760_654_912_24;
pub const GEN5_B: f64 = 0.005_494_072_432_257_808;
pub const GEN5_C: f64 = 0.530_013_339_229_193_9;
pub const GEN5_D: f64 = 8.283_605_932_402_494;
pub const GEN5_E: f64 = 0.092_465_753_424_657_53;
/// Scene light below this uses the linear branch.
pub const GEN5_LIN_CUT: f64 = 0.005;
/// The code value at which the two branches meet (`D * LIN_CUT + E`).
pub const GEN5_LOG_CUT: f64 = GEN5_D * GEN5_LIN_CUT + GEN5_E;

/// Middle grey in scene-linear light.
pub const MID_GREY_SCENE: f64 = 0.18;
/// The gamma of the film's straight-line part, as density per log10 exposure. Kodak's VISION3 data
/// sheets give the sensitometric curves only as graphs, so this is an engineering estimate for
/// camera negative film in ECN-2 (about 0.6), not a measured value; the exposure slider covers
/// differences between stocks and pushed or pulled development.
pub const FILM_GAMMA: f64 = 0.6;
/// The stops across the black to white range are kept between these, so a flat or a very contrasty
/// frame cannot give an absurd curve.
pub const MIN_RANGE_STOPS: f64 = 2.0;
pub const MAX_RANGE_STOPS: f64 = 16.0;
/// How far the 16-point curve may be from the exact function between the black and the white point,
/// in code value (0 to 1); about one step of an 8-bit encoding.
pub const CURVE_TOLERANCE: f64 = 0.005;
/// Above the white point the curve continues with its end slope while the exact function bends
/// away, so it is allowed more room there, up to 10 % of the range above the white point.
pub const HEADROOM_TOLERANCE: f64 = 0.02;

/// Scene-linear light to a Film Gen 5 code value.
pub fn gen5_encode(light: f64) -> f64 {
    if light < GEN5_LIN_CUT {
        GEN5_D * light + GEN5_E
    } else {
        GEN5_A * (light + GEN5_B).ln() + GEN5_C
    }
}

/// A Film Gen 5 code value back to scene-linear light.
pub fn gen5_decode(code: f64) -> f64 {
    if code < GEN5_LOG_CUT {
        (code - GEN5_E) / GEN5_D
    } else {
        ((code - GEN5_C) / GEN5_A).exp() - GEN5_B
    }
}

/// The sRGB decoding of a display value: linear light for a code shown as it is.
pub fn srgb_decode(value: f64) -> f64 {
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// The stops of scene light across the frame's black to white range.
pub fn range_stops(levels: &Invert) -> f64 {
    let span: f64 = (0..3)
        .map(|c| f64::from(levels.white[c] - levels.black[c]))
        .sum::<f64>()
        / 3.0;
    (span / (FILM_GAMMA * std::f64::consts::LOG10_2)).clamp(MIN_RANGE_STOPS, MAX_RANGE_STOPS)
}

/// The exact code value for inversion output `p` (0 black point, 1 white point).
pub fn film_log_code(p: f64, stops: f64) -> f64 {
    gen5_encode(MID_GREY_SCENE * 2f64.powf((p - 0.5) * stops))
}

/// The master curve of the Film log look for a frame with these levels: 16 points from the black
/// to the white point, in linear light so that the display encoding shows the code values.
pub fn film_log_curve(levels: &Invert) -> Vec<CurvePoint> {
    let stops = range_stops(levels);
    (0..CURVE_POINTS_MAX)
        .map(|i| {
            let p = i as f64 / (CURVE_POINTS_MAX - 1) as f64;
            [p as f32, srgb_decode(film_log_code(p, stops)) as f32]
        })
        .collect()
}

/// The master curve a look gives a frame with these levels; none for [`Look::Neutral`].
pub fn look_curve(look: Look, levels: &Invert) -> Option<Vec<CurvePoint>> {
    match look {
        Look::Neutral => None,
        Look::FilmLog => Some(film_log_curve(levels)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Curve;

    fn levels(span: f32) -> Invert {
        Invert {
            black: [0.1; 3],
            white: [0.1 + span; 3],
        }
    }

    fn srgb_encode(linear: f64) -> f64 {
        if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        }
    }

    #[test]
    fn the_transfer_function_matches_its_published_values() {
        // The documented example: 18 % grey.
        assert!((gen5_encode(0.18) - 0.383_561_64).abs() < 1e-6);
        // The linear branch at black, and where it ends.
        assert!((gen5_encode(0.0) - GEN5_E).abs() < 1e-12);
        assert!((gen5_encode(GEN5_LIN_CUT) - GEN5_LOG_CUT).abs() < 1e-3);
        assert!((GEN5_LOG_CUT - 0.133_883_783_086_67).abs() < 1e-9);
        // The two branches meet: no jump at the cut, to the precision of the published constants.
        let (below, above) = (gen5_encode(GEN5_LIN_CUT - 1e-9), gen5_encode(GEN5_LIN_CUT));
        assert!((below - above).abs() < 1e-4, "{below} vs {above}");
        // White (1.0) is well inside the code range, and the curve rises.
        assert!(gen5_encode(1.0) > 0.5 && gen5_encode(1.0) < 0.7);
        assert!(gen5_encode(16.0) > gen5_encode(1.0));
    }

    #[test]
    fn decoding_undoes_encoding_on_both_branches() {
        for light in [0.0, 0.001, 0.004_999, 0.005, 0.02, 0.18, 1.0, 8.0, 40.0] {
            let back = gen5_decode(gen5_encode(light));
            assert!(
                (back - light).abs() < 1e-6 * light.max(1.0),
                "{light}: {back}"
            );
        }
    }

    #[test]
    fn the_range_in_stops_follows_the_levels_within_its_limits() {
        // A density span of 1.2 at gamma 0.6 is 6.6 stops.
        assert!((range_stops(&levels(1.2)) - 1.2 / (0.6 * std::f64::consts::LOG10_2)).abs() < 1e-3);
        assert_eq!(range_stops(&levels(0.001)), MIN_RANGE_STOPS);
        assert_eq!(range_stops(&levels(9.0)), MAX_RANGE_STOPS);
    }

    #[test]
    fn the_middle_of_the_range_is_middle_grey_and_the_ends_leave_headroom() {
        let stops = range_stops(&levels(1.2));
        assert!((film_log_code(0.5, stops) - gen5_encode(0.18)).abs() < 1e-9);
        let (low, high) = (film_log_code(0.0, stops), film_log_code(1.0, stops));
        assert!(low > 0.1 && low < 0.35, "black ends at {low}");
        assert!(high > 0.5 && high < 0.75, "white ends at {high}");
    }

    #[test]
    fn the_sixteen_points_follow_the_exact_encoding_within_the_tolerance() {
        for span in [0.5, 0.9, 1.2, 1.8, 2.6] {
            let lv = levels(span);
            let stops = range_stops(&lv);
            let points = film_log_curve(&lv);
            assert_eq!(points.len(), CURVE_POINTS_MAX);
            let curve = Curve::new(&points).unwrap();
            let mut worst = 0.0f64;
            let mut worst_headroom = 0.0f64;
            for k in 0..=1100 {
                let p = f64::from(k) / 1000.0;
                let shown = srgb_encode(f64::from(curve.eval(p as f32)));
                let error = (shown - film_log_code(p, stops)).abs();
                if p <= 1.0 {
                    worst = worst.max(error);
                } else {
                    worst_headroom = worst_headroom.max(error);
                }
            }
            assert!(
                worst_headroom <= HEADROOM_TOLERANCE,
                "span {span}: headroom {worst_headroom}"
            );
            assert!(worst <= CURVE_TOLERANCE, "span {span}: worst {worst}");
        }
    }

    #[test]
    fn the_curve_is_valid_for_a_recipe_and_neutral_adds_nothing() {
        let curve = film_log_curve(&levels(1.2));
        assert!(curve
            .windows(2)
            .all(|w| w[0][0] < w[1][0] && w[0][1] < w[1][1]));
        assert!(curve
            .iter()
            .all(|p| (0.0..=1.0).contains(&p[0]) && (0.0..=1.0).contains(&p[1])));
        assert_eq!(look_curve(Look::FilmLog, &levels(1.2)), Some(curve));
        assert_eq!(look_curve(Look::Neutral, &levels(1.2)), None);
    }
}
