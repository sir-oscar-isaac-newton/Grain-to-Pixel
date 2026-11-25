//! Colours above white: how a linear Rec.2020 pixel with a channel above 1 is brought into
//! `[0, 1]` before the output conversion (ADR 0019, docs/specification.md section 4.2).
//!
//! Clipping each channel on its own turns a bright saturated colour pale and shifts its hue, since
//! the ratio between its channels is lost. This rule keeps the ratio: the pixel is scaled so that
//! its largest channel is 1, then blended towards white by an amount that grows with how far the
//! largest channel was above 1. The screen conversion and every export, grey included, call this
//! one function, so they agree.

/// The softness of the roll-off to white, in the units of the largest channel above 1.
///
/// The blend towards white is `(m - 1) / (m - 1 + K)` for a largest channel `m`. At `m = 1 + K`
/// the colour is half way to white; the larger `K` is, the longer a bright colour keeps its
/// saturation.
pub const HIGHLIGHT_KNEE: f32 = 1.0;

/// A channel as the output stage takes it: NaN becomes 0, a negative value (negative infinity
/// included) becomes 0, positive infinity becomes the largest finite value.
#[inline]
fn finite_non_negative(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, f32::MAX)
    }
}

/// Brings a linear Rec.2020 pixel into `[0, 1]` in each channel, keeping its hue.
///
/// - A pixel with no channel above 1 is only cleaned (NaN to 0, negative to 0), which is exactly
///   what clipping each channel to `[0, 1]` gives, bit for bit.
/// - Otherwise the pixel is divided by its largest channel `m`, so the ratios between the channels
///   (the hue) stay and the largest channel is exactly 1, then each channel is moved towards 1 by
///   `t = (m - 1) / (m - 1 + K)` with `K` = [`HIGHLIGHT_KNEE`]. Saturation falls towards white as
///   `m` grows, and a neutral pixel reaches exactly `[1, 1, 1]`.
///
/// The result is always finite and inside `[0, 1]`. It is continuous at `m = 1`, and scaling a
/// pixel up never darkens a channel. Raising one channel alone can lower another a little, since
/// the pixel is divided by its largest channel.
#[inline]
pub fn bring_into_range(rgb: [f32; 3]) -> [f32; 3] {
    let p = rgb.map(finite_non_negative);
    let m = p[0].max(p[1]).max(p[2]);
    if m <= 1.0 {
        return p;
    }
    // `m - 1 + K` cannot overflow: for the largest finite `m`, `m - 1` rounds back to `m`, and
    // adding `K` rounds to `m` again.
    let over = m - 1.0;
    let t = over / (over + HIGHLIGHT_KNEE);
    p.map(|v| {
        let c = v / m;
        (c + (1.0 - c) * t).clamp(0.0, 1.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule used until P-32: each channel clipped on its own, NaN to 0.
    fn old_clamp(rgb: [f32; 3]) -> [f32; 3] {
        rgb.map(|v| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) })
    }

    const SPECIAL: [f32; 12] = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -0.0,
        0.0,
        -3.0,
        0.5,
        1.0,
        7.0,
        f32::MAX,
        f32::MIN_POSITIVE,
        1.0e-30,
    ];

    /// Hue angle in degrees as HSV defines it, on the linear values.
    fn hue(p: [f32; 3]) -> f64 {
        let [r, g, b] = p.map(f64::from);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        assert!(d > 0.0, "grey has no hue");
        let h = if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h * 60.0
    }

    fn hue_gap(a: f64, b: f64) -> f64 {
        let d = (a - b).abs() % 360.0;
        d.min(360.0 - d)
    }

    #[test]
    fn pixels_with_no_channel_above_one_are_unchanged_bit_for_bit() {
        let below_one = 1.0f32.to_bits() - 1;
        let mut values: Vec<f32> = vec![
            0.0,
            -0.0,
            1.0,
            f32::from_bits(below_one),
            f32::from_bits(below_one - 1),
            f32::NAN,
            -1.0,
            -1.0e-20,
            f32::NEG_INFINITY,
            f32::MIN_POSITIVE,
        ];
        values.extend((0..=200).map(|i| i as f32 / 200.0));
        values.extend((0..=40).map(|i| -(i as f32) / 20.0));
        let mut checked = 0u32;
        for &r in &values {
            for &g in &values {
                for &b in &values {
                    let got = bring_into_range([r, g, b]);
                    let want = old_clamp([r, g, b]);
                    for c in 0..3 {
                        assert_eq!(
                            got[c].to_bits(),
                            want[c].to_bits(),
                            "{:?} channel {c}",
                            [r, g, b]
                        );
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 1_000_000);
    }

    #[test]
    fn bright_saturated_colours_keep_their_hue() {
        let mut colours = vec![
            [1.8, 0.9, 0.2],
            [0.4, 1.8, 0.6],
            [0.6, 2.7, 0.9],
            [1.8, 0.3, 1.5],
        ];
        for k in 1..=8 {
            for base in [
                [1.0, 0.5, 0.1],
                [0.2, 1.0, 0.4],
                [0.9, 0.1, 1.0],
                [0.1, 0.4, 1.0],
            ] {
                colours.push(base.map(|v| v * k as f32));
            }
        }
        for p in colours {
            let out = bring_into_range(p);
            assert!(
                hue_gap(hue(p), hue(out)) < 0.1,
                "{p:?} -> {out:?}: hue {} to {}",
                hue(p),
                hue(out)
            );
        }
    }

    #[test]
    fn the_largest_channel_of_a_bright_pixel_is_exactly_one() {
        for p in [[1.8, 0.9, 0.2], [0.6, 2.7, 0.9], [1.0001, 0.2, 0.1]] {
            let out = bring_into_range(p);
            assert_eq!(out[0].max(out[1]).max(out[2]), 1.0, "{p:?}");
        }
    }

    #[test]
    fn neutral_above_one_reaches_exactly_white() {
        for v in [1.0f32, 1.000_001, 1.5, 4.0, 1000.0, f32::MAX, f32::INFINITY] {
            assert_eq!(bring_into_range([v; 3]), [1.0; 3], "{v}");
        }
    }

    #[test]
    fn the_result_is_always_finite_and_inside_the_range() {
        for r in SPECIAL {
            for g in SPECIAL {
                for b in SPECIAL {
                    let out = bring_into_range([r, g, b]);
                    for v in out {
                        assert!(
                            v.is_finite() && (0.0..=1.0).contains(&v),
                            "{:?} -> {out:?}",
                            [r, g, b]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_brighter_pixel_is_never_darker_in_any_channel() {
        let bases = [
            [0.2f32, 0.5, 0.9],
            [1.8, 0.9, 0.2],
            [0.6, 2.7, 0.9],
            [0.0, 0.0, 1.2],
            [1.0, 1.0, 1.0],
        ];
        for base in bases {
            let mut last = [0.0f32; 3];
            for step in 1..=400 {
                let k = step as f32 * 0.05;
                let out = bring_into_range(base.map(|v| v * k));
                for c in 0..3 {
                    // Allow the rounding of one division and one blend.
                    assert!(
                        out[c] >= last[c] - 1.0e-6,
                        "{base:?} x{k}: channel {c} fell from {} to {}",
                        last[c],
                        out[c]
                    );
                }
                last = out;
            }
        }
    }

    #[test]
    fn the_rule_is_continuous_at_one() {
        let eps = f32::EPSILON;
        for base in [[1.0f32, 0.5, 0.1], [0.3, 1.0, 0.8], [1.0, 1.0, 0.2]] {
            let at = bring_into_range(base);
            let above = bring_into_range(base.map(|v| v * (1.0 + 4.0 * eps)));
            for c in 0..3 {
                assert!(
                    (at[c] - above[c]).abs() < 1.0e-5,
                    "{base:?} channel {c}: {} vs {}",
                    at[c],
                    above[c]
                );
            }
        }
    }
}
