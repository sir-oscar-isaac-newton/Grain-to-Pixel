//! Tone curves: a monotone cubic through the control points, sampled into a lookup table.
//!
//! The spline is the Fritsch-Carlson monotone cubic (PCHIP): it never overshoots, so monotone
//! control points always give a monotone curve. Outside the control points the curve is held flat,
//! except above an end point at x = 1, where it continues with the end slope so that values above
//! 1.0 (highlight headroom) keep their order. Values below 0 map to the curve's value at 0.

use gtp_core::recipe::CurvePoint;
use gtp_core::{AppError, ErrorCode};

/// Number of entries in the lookup table over `[0, 1]`.
pub const LUT_SIZE: usize = 4096;

/// A ready-to-apply tone curve.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    lut: Vec<f32>,
    /// Slope used above x = 1.
    tail_slope: f32,
    identity: bool,
}

fn invalid(message: &str, points: &[CurvePoint]) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message).with_details(format!("{points:?}"))
}

/// Tangents of the monotone cubic (Fritsch-Carlson with the PCHIP weighting).
fn tangents(xs: &[f32], ys: &[f32]) -> Vec<f32> {
    let n = xs.len();
    let h: Vec<f32> = xs.windows(2).map(|w| w[1] - w[0]).collect();
    let d: Vec<f32> = ys
        .windows(2)
        .zip(&h)
        .map(|(w, h)| (w[1] - w[0]) / h)
        .collect();
    let mut m = vec![0.0; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        if d[i - 1] * d[i] > 0.0 {
            let w1 = 2.0 * h[i] + h[i - 1];
            let w2 = h[i] + 2.0 * h[i - 1];
            m[i] = (w1 + w2) / (w1 / d[i - 1] + w2 / d[i]);
        }
    }
    m
}

impl Curve {
    /// The curve that changes nothing.
    pub fn identity() -> Self {
        Self {
            lut: (0..LUT_SIZE)
                .map(|i| i as f32 / (LUT_SIZE - 1) as f32)
                .collect(),
            tail_slope: 1.0,
            identity: true,
        }
    }

    /// Builds a curve from control points `[input, output]`. An empty list is the identity.
    /// Points must be finite, inside the unit square and strictly increasing in input, and there
    /// must be at least two (`Recipe::clamp` already sorts and limits stored curves).
    pub fn new(points: &[CurvePoint]) -> Result<Self, AppError> {
        if points.is_empty() {
            return Ok(Self::identity());
        }
        if points.len() < 2 {
            return Err(invalid("A curve needs at least two points.", points));
        }
        let in_square = points
            .iter()
            .all(|p| p.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        let increasing = points.windows(2).all(|w| w[0][0] < w[1][0]);
        if !in_square || !increasing {
            return Err(invalid(
                "Curve points must lie in the unit square with increasing inputs.",
                points,
            ));
        }

        let xs: Vec<f32> = points.iter().map(|p| p[0]).collect();
        let ys: Vec<f32> = points.iter().map(|p| p[1]).collect();
        let m = tangents(&xs, &ys);
        let last = xs.len() - 1;
        let lut: Vec<f32> = (0..LUT_SIZE)
            .map(|i| {
                let x = i as f32 / (LUT_SIZE - 1) as f32;
                if x <= xs[0] {
                    return ys[0];
                }
                if x >= xs[last] {
                    return ys[last];
                }
                let k = xs.windows(2).position(|w| x < w[1]).unwrap_or(last - 1);
                let h = xs[k + 1] - xs[k];
                let t = (x - xs[k]) / h;
                let (t2, t3) = (t * t, t * t * t);
                let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
                let h10 = t3 - 2.0 * t2 + t;
                let h01 = -2.0 * t3 + 3.0 * t2;
                let h11 = t3 - t2;
                (h00 * ys[k] + h10 * h * m[k] + h01 * ys[k + 1] + h11 * h * m[k + 1])
                    .clamp(0.0, 1.0)
            })
            .collect();
        let tail_slope = if xs[last] >= 1.0 {
            m[last].max(0.0)
        } else {
            0.0
        };
        let identity = (tail_slope - 1.0).abs() < 1e-6
            && lut
                .iter()
                .enumerate()
                .all(|(i, v)| (v - i as f32 / (LUT_SIZE - 1) as f32).abs() < 1e-6);
        Ok(Self {
            lut,
            tail_slope,
            identity,
        })
    }

    /// True when the curve changes nothing (it is skipped).
    pub fn is_identity(&self) -> bool {
        self.identity
    }

    /// The curve's value at `x`, interpolating the lookup table linearly.
    pub fn eval(&self, x: f32) -> f32 {
        let last = LUT_SIZE - 1;
        if x.is_nan() || x <= 0.0 {
            return self.lut[0];
        }
        if x >= 1.0 {
            return self.lut[last] + (x - 1.0) * self.tail_slope;
        }
        let position = x * last as f32;
        let i = (position as usize).min(last - 1);
        let fraction = position - i as f32;
        self.lut[i] + (self.lut[i + 1] - self.lut[i]) * fraction
    }
}

#[cfg(test)]
mod tests {
    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn the_default_and_empty_curves_are_the_identity() {
        for curve in [
            Curve::new(&[[0.0, 0.0], [1.0, 1.0]]).unwrap(),
            Curve::new(&[]).unwrap(),
        ] {
            assert!(curve.is_identity());
            for x in [0.0, 0.123, 0.5, 0.999, 1.0, 1.7, 4.0] {
                assert!((curve.eval(x) - x).abs() < 1e-6, "{x}");
            }
        }
    }

    #[test]
    fn the_curve_passes_through_its_control_points() {
        let points = [
            [0.0, 0.0],
            [0.25, 0.15],
            [0.5, 0.5],
            [0.75, 0.85],
            [1.0, 1.0],
        ];
        let curve = Curve::new(&points).unwrap();
        assert!(!curve.is_identity());
        for [x, y] in points {
            assert!(
                (curve.eval(x) - y).abs() < 1e-3,
                "({x}, {y}) -> {}",
                curve.eval(x)
            );
        }
    }

    #[test]
    fn outside_the_points_the_curve_is_held_or_continues_into_headroom() {
        let held = Curve::new(&[[0.2, 0.1], [0.8, 0.9]]).unwrap();
        assert_eq!(held.eval(0.0), 0.1);
        assert_eq!(held.eval(-5.0), 0.1);
        assert!((held.eval(0.95) - 0.9).abs() < 1e-6);
        assert!(
            (held.eval(3.0) - 0.9).abs() < 1e-6,
            "no end point at 1: flat"
        );

        let open = Curve::new(&[[0.0, 0.0], [0.5, 0.6], [1.0, 1.0]]).unwrap();
        assert!(
            open.eval(1.5) > open.eval(1.0),
            "an end point at 1 keeps headroom"
        );
        assert_eq!(open.eval(f32::NAN), 0.0);
    }

    #[test]
    fn invalid_points_are_rejected() {
        for points in [
            vec![[0.5, 0.5]],
            vec![[0.0, 0.0], [0.0, 1.0]],
            vec![[0.6, 0.0], [0.4, 1.0]],
            vec![[0.0, 0.0], [1.2, 1.0]],
            vec![[0.0, -0.1], [1.0, 1.0]],
            vec![[f32::NAN, 0.0], [1.0, 1.0]],
        ] {
            assert_eq!(
                Curve::new(&points).unwrap_err().code,
                ErrorCode::InvalidInput,
                "{points:?}"
            );
        }
    }

    proptest! {
        /// Monotone control points give a monotone curve (no overshoot), and every value stays
        /// inside [0, 1] on the unit interval.
        #[test]
        fn monotone_points_give_a_monotone_curve(
            mut xs in vec(0.0f32..=1.0, 2..12),
            mut ys in vec(0.0f32..=1.0, 12),
        ) {
            xs.sort_by(f32::total_cmp);
            xs.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
            prop_assume!(xs.len() >= 2);
            ys.truncate(xs.len());
            ys.sort_by(f32::total_cmp);
            let points: Vec<CurvePoint> = xs.iter().zip(&ys).map(|(x, y)| [*x, *y]).collect();
            let curve = Curve::new(&points).unwrap();
            let mut previous = f32::MIN;
            for i in 0..=2000 {
                let value = curve.eval(i as f32 / 1000.0);
                prop_assert!(value >= previous - 1e-6, "not monotone at {}", i as f32 / 1000.0);
                prop_assert!(value.is_finite());
                previous = value;
            }
        }
    }
}
