//! The Frontier-style look (P-20, decisions/0009-frontier-look.md).
//!
//! Trichroma Lab imitates the Fuji Frontier SP-3000 minilab: a contrasty lab-print rendition with
//! raised saturation. The look is two things written into the automatic recipe: the master tone
//! curve and the saturation. The inversion's output `P` runs from 0 at the frame's black point to 1
//! at its white point (the automatic levels normalize every frame), so one curve serves them all:
//!
//! ```text
//! s(P)   = 1 / (1 + exp(-slope * (P - pivot)))             a sigmoid, steeper with `slope`
//! y(P)   = ((s(P) - s(0)) / (s(1) - s(0))) ^ gamma         0 at black, 1 at white; gamma shapes the mids
//! master = srgb_decode(y)                                  linear light, so the display shows y
//! ```
//!
//! The four numbers ([`FrontierParams`]) are found by `benchmark fit` against the author's
//! reference pairs; the values below are its result (see `docs/research/benchmark.md`).

use gtp_core::recipe::{CurvePoint, CURVE_POINTS_MAX};

use crate::srgb_decode;

/// The fitted values: the sigmoid's slope and pivot, the gamma after it, and the saturation.
pub const FRONTIER_SLOPE: f64 = 5.719;
pub const FRONTIER_PIVOT: f64 = 0.495;
pub const FRONTIER_GAMMA: f64 = 1.284;
pub const FRONTIER_SATURATION: f64 = 0.562;

/// What the fit may choose between: the curve and the saturation stay inside these.
pub const SLOPE_RANGE: (f64, f64) = (0.5, 14.0);
pub const PIVOT_RANGE: (f64, f64) = (0.1, 0.9);
pub const GAMMA_RANGE: (f64, f64) = (0.4, 2.6);
pub const SATURATION_RANGE: (f64, f64) = (0.2, 1.8);

/// The four numbers of a Frontier-style look.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrontierParams {
    pub slope: f64,
    pub pivot: f64,
    pub gamma: f64,
    pub saturation: f64,
}

impl FrontierParams {
    /// The fitted look.
    pub const FITTED: FrontierParams = FrontierParams {
        slope: FRONTIER_SLOPE,
        pivot: FRONTIER_PIVOT,
        gamma: FRONTIER_GAMMA,
        saturation: FRONTIER_SATURATION,
    };

    /// The same four numbers as an array, in the order slope, pivot, gamma, saturation.
    pub fn to_array(self) -> [f64; 4] {
        [self.slope, self.pivot, self.gamma, self.saturation]
    }

    /// From an array in that order, each number brought inside its range.
    pub fn from_array(values: [f64; 4]) -> Self {
        let clamp = |v: f64, (lo, hi): (f64, f64)| v.clamp(lo, hi);
        Self {
            slope: clamp(values[0], SLOPE_RANGE),
            pivot: clamp(values[1], PIVOT_RANGE),
            gamma: clamp(values[2], GAMMA_RANGE),
            saturation: clamp(values[3], SATURATION_RANGE),
        }
    }
}

fn sigmoid(p: f64, slope: f64, pivot: f64) -> f64 {
    1.0 / (1.0 + (-slope * (p - pivot)).exp())
}

/// The display value (0 to 1) for inversion output `p`; 0 at the black point, 1 at the white point.
pub fn frontier_code(p: f64, params: &FrontierParams) -> f64 {
    let (low, high) = (
        sigmoid(0.0, params.slope, params.pivot),
        sigmoid(1.0, params.slope, params.pivot),
    );
    let scaled = ((sigmoid(p, params.slope, params.pivot) - low) / (high - low)).max(0.0);
    scaled.powf(params.gamma)
}

/// The master curve of a look with these numbers: [`CURVE_POINTS_MAX`] points from the black to the
/// white point, in linear light so that the display encoding shows [`frontier_code`]. The points are
/// closer together near both ends, where the sigmoid bends most.
pub fn frontier_curve(params: &FrontierParams) -> Vec<CurvePoint> {
    (0..CURVE_POINTS_MAX)
        .map(|i| {
            let t = i as f64 / (CURVE_POINTS_MAX - 1) as f64;
            // Half a cosine ramp mixed into an even spacing.
            let p = 0.5 * t + 0.5 * (0.5 - 0.5 * (std::f64::consts::PI * t).cos());
            [p as f32, srgb_decode(frontier_code(p, params)) as f32]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Curve;

    fn srgb_encode(linear: f64) -> f64 {
        if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        }
    }

    const SAMPLE: [FrontierParams; 4] = [
        FrontierParams::FITTED,
        FrontierParams {
            slope: 3.0,
            pivot: 0.45,
            gamma: 1.0,
            saturation: 1.0,
        },
        FrontierParams {
            slope: 10.0,
            pivot: 0.55,
            gamma: 1.6,
            saturation: 1.3,
        },
        FrontierParams {
            slope: 1.0,
            pivot: 0.5,
            gamma: 0.8,
            saturation: 0.9,
        },
    ];

    #[test]
    fn the_code_runs_from_black_to_white_and_rises() {
        for params in SAMPLE {
            assert!(frontier_code(0.0, &params).abs() < 1e-12, "{params:?}");
            assert!(
                (frontier_code(1.0, &params) - 1.0).abs() < 1e-12,
                "{params:?}"
            );
            let mut last = -1.0;
            for k in 0..=100 {
                let code = frontier_code(f64::from(k) / 100.0, &params);
                assert!(code >= last, "{params:?} at {k}");
                last = code;
            }
        }
        // A steeper slope is a contrastier curve: darker shadows and brighter highlights.
        let soft = FrontierParams {
            slope: 2.0,
            ..FrontierParams::FITTED
        };
        let hard = FrontierParams {
            slope: 10.0,
            ..FrontierParams::FITTED
        };
        assert!(frontier_code(0.2, &hard) < frontier_code(0.2, &soft));
        assert!(frontier_code(0.8, &hard) > frontier_code(0.8, &soft));
        // The gamma moves the middle and leaves the ends.
        let darker = FrontierParams {
            gamma: 2.0,
            ..FrontierParams::FITTED
        };
        assert!(frontier_code(0.5, &darker) < frontier_code(0.5, &FrontierParams::FITTED));
    }

    #[test]
    fn the_sixteen_points_follow_the_exact_code_within_the_tolerance() {
        for params in SAMPLE {
            let points = frontier_curve(&params);
            assert_eq!(points.len(), CURVE_POINTS_MAX);
            assert!(points
                .windows(2)
                .all(|w| w[0][0] < w[1][0] && w[0][1] <= w[1][1]));
            let curve = Curve::new(&points).unwrap();
            let worst = (0..=1000)
                .map(|k| {
                    let p = f64::from(k) / 1000.0;
                    let shown = srgb_encode(f64::from(curve.eval(p as f32)));
                    (shown - frontier_code(p, &params)).abs()
                })
                .fold(0.0, f64::max);
            assert!(worst <= FIT_TOLERANCE, "{params:?}: worst {worst}");
        }
    }

    /// How far the curve may be from the exact code between black and white, in code value.
    const FIT_TOLERANCE: f64 = 0.01;

    #[test]
    fn numbers_are_brought_inside_their_ranges_and_round_trip() {
        let wild = FrontierParams::from_array([99.0, -3.0, 0.0, 9.0]);
        assert_eq!(wild.slope, SLOPE_RANGE.1);
        assert_eq!(wild.pivot, PIVOT_RANGE.0);
        assert_eq!(wild.gamma, GAMMA_RANGE.0);
        assert_eq!(wild.saturation, SATURATION_RANGE.1);
        let p = FrontierParams::FITTED;
        assert_eq!(FrontierParams::from_array(p.to_array()), p);
        // The fitted values are inside the ranges the fit works in.
        assert!((SLOPE_RANGE.0..=SLOPE_RANGE.1).contains(&FRONTIER_SLOPE));
        assert!((PIVOT_RANGE.0..=PIVOT_RANGE.1).contains(&FRONTIER_PIVOT));
        assert!((GAMMA_RANGE.0..=GAMMA_RANGE.1).contains(&FRONTIER_GAMMA));
        assert!((SATURATION_RANGE.0..=SATURATION_RANGE.1).contains(&FRONTIER_SATURATION));
    }
}
