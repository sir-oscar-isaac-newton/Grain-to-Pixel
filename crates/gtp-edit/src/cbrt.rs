//! Our own cube root for `f32`, used by the Oklab conversion (ADR 0022).
//!
//! The platform's `cbrt` is a library call that is not promised to give the same last bits on every
//! system, and it is slower; the measured figures are in the ADR. This function uses integer
//! operations and IEEE `+`, `*` and `/` only, so its result is the same on every platform that has
//! IEEE 754 arithmetic (no fused multiply-add, no maths-library call; Rust never fuses a multiply
//! and an add on its own).
//!
//! **Method.** For a finite non-zero `x`, with `a = |x|`:
//! 1. A subnormal `a` is multiplied by 2^24 (exact), and the result is divided by 2^8 at the end
//!    (also exact: 24 is a multiple of 3 and the cube root of 2^24 is 2^8).
//! 2. The seed comes from the bits of `a` read as an integer: `bits / 3 + CBRT_SEED_OFFSET`, read back
//!    as an `f32`. The exponent field of a float is its base-2 logarithm in steps of 2^23 and the
//!    mantissa field is nearly linear in the logarithm, so a third of the bits is nearly a third of
//!    the logarithm, which is the cube root. [`CBRT_SEED_OFFSET`] is derived below.
//! 3. Two Halley steps in `f64`: `t' = t (2a + t^3) / (a + 2 t^3)`. If `t = c (1 + e)` for the true
//!    root `c`, one step gives `e' = 2/3 e^3`, so a seed within 3.2 % of the root is within about
//!    2e-5 after the first step and about 6e-15 after the second (7.52e-15 is the largest measured
//!    with `f64` rounding). That is not a proof of correct rounding: what matters is the distance of
//!    the true root from the midpoint between two `f32` values, and the closest approach over
//!    `[1, 8)` is 2^-49.07 relative (the input `0x400353B5`, root `0x3FA2A7BC`), smaller than the
//!    worst two-step error of 2^-46.92. Correct rounding therefore holds because the exhaustive
//!    test (`every_float_in_one_to_eight_is_correctly_rounded`) checked every input, not by a
//!    margin. Changing the seed, the number of steps or the order of the operations requires
//!    running that test again.
//! 4. The result is rounded to `f32` once, and the sign of `x` is put back.
//!
//! Zero keeps its sign, an infinity stays an infinity and a NaN is returned as it came (a
//! signalling NaN too, unchanged); the library may return another payload, so the payload of a NaN
//! is not part of the promise.
//!
//! **Why `[1, 8)` is enough to test.** Multiplying `a` by 2^(3k) adds `k` to the exponent of the
//! root. In the seed, `(bits + 3k 2^23) / 3 = bits / 3 + k 2^23` exactly in integer division, so
//! the seed is multiplied by exactly 2^k; both Halley steps are homogeneous (`a` scales by 2^(3k),
//! `t` by 2^k, every product and quotient by a power of two, with no overflow or underflow in
//! `f64`); and the final rounding is the same for a result that stays normal. So the result for any
//! normal `x` is the result for the `y` in `[1, 8)` with `x = y 2^(3k)`, times 2^k, bit for bit. A
//! test checks this on a strided set, and the exhaustive test covers every `y` in `[1, 8)`.
//!
//! **Seed offset.** The ignored test `search_the_seed_offset` scans the offsets and keeps the one
//! with the smallest largest relative error of the seed over `[1, 8)`. The first guess is the
//! exact value of the logarithm argument: for `x = 2^e (1 + m)` the bits are
//! `(e + 127) 2^23 + m 2^23`, the root has bits near `(bits - 127 2^23) / 3 + 127 2^23`, which is
//! `bits / 3 + 2/3 127 2^23`, and the straight line of the mantissa is then shifted to balance the
//! largest error on both sides of the curve of `log2(1 + m)`.

/// Mask of the sign bit.
const SIGN_MASK: u32 = 0x8000_0000;
/// Mask of everything but the sign bit.
const ABS_MASK: u32 = 0x7FFF_FFFF;
/// The bits of the smallest normal `f32`; anything below is subnormal.
const MIN_NORMAL_BITS: u32 = 0x0080_0000;
/// The bits of the infinity; anything from here on is an infinity or a NaN.
const INFINITY_BITS: u32 = 0x7F80_0000;
/// 2^24, the exact factor that brings a subnormal into the normal range.
const SUBNORMAL_SCALE: f32 = 16_777_216.0;
/// 2^-8, the cube root of 1 / [`SUBNORMAL_SCALE`], undoing the scaling on the root.
const SUBNORMAL_UNSCALE: f64 = 1.0 / 256.0;

/// Added to a third of the bits of `|x|` to make the seed (see the module docs). Found by
/// `search_the_seed_offset`: with it the seed is within 3.2 % of the root on all of `[1, 8)`.
const CBRT_SEED_OFFSET: u32 = 0x2A51_06C2;

/// One Halley step towards the cube root of `a`: `t (2a + t^3) / (a + 2 t^3)`. The order of the
/// operations is fixed, so is the result.
#[inline(always)]
fn halley(t: f64, a: f64) -> f64 {
    let t3 = t * t * t;
    t * (2.0 * a + t3) / (a + 2.0 * t3)
}

/// The cube root of a normal positive `a`, before rounding to `f32`.
#[inline(always)]
fn root(a: f32) -> f64 {
    let seed = f32::from_bits(a.to_bits() / 3 + CBRT_SEED_OFFSET);
    let a = f64::from(a);
    halley(halley(f64::from(seed), a), a)
}

/// The cube root of `x`, the same on every platform, with a result that is correctly rounded (the
/// nearest `f32` to the true root) on every finite input, and so always within 0.5 ulp.
/// Defined for negative values (the sign is kept); zero keeps its sign; infinities and NaN come
/// back as they are.
#[inline]
pub(crate) fn cbrt(x: f32) -> f32 {
    let bits = x.to_bits();
    let magnitude = bits & ABS_MASK;
    if magnitude == 0 || magnitude >= INFINITY_BITS {
        return x;
    }
    let a = f32::from_bits(magnitude);
    let root = if magnitude < MIN_NORMAL_BITS {
        root(a * SUBNORMAL_SCALE) * SUBNORMAL_UNSCALE
    } else {
        root(a)
    };
    f32::from_bits((root as f32).to_bits() | (bits & SIGN_MASK))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bits of the smallest and largest positive finite `f32`.
    const MAX_FINITE_BITS: u32 = 0x7F7F_FFFF;
    /// The bits of 1.0 and 8.0: the range that stands for all normals.
    const ONE_BITS: u32 = 0x3F80_0000;
    const EIGHT_BITS: u32 = 0x4100_0000;

    // ---- exact reference ------------------------------------------------------------------

    /// A finite positive `f32` as `(mantissa, exponent)`: the value is `mantissa * 2^exponent`.
    fn decompose(x: f32) -> (u128, i32) {
        let bits = x.to_bits();
        let fraction = u128::from(bits & 0x007F_FFFF);
        let field = ((bits >> 23) & 0xFF) as i32;
        if field == 0 {
            (fraction, -149)
        } else {
            (fraction | 0x0080_0000, field - 150)
        }
    }

    /// Whether `a * 2^ea` is less than `b * 2^eb`, in exact arithmetic.
    fn less(a: u128, ea: i32, b: u128, eb: i32) -> bool {
        if ea >= eb {
            let shift = (ea - eb) as u32;
            assert!(a.leading_zeros() > shift, "the comparison would overflow");
            (a << shift) < b
        } else {
            let shift = (eb - ea) as u32;
            assert!(b.leading_zeros() > shift, "the comparison would overflow");
            a < (b << shift)
        }
    }

    /// The cube of the midpoint between two neighbouring floats, as `(cube, exponent)`.
    fn midpoint_cubed(lo: f32, hi: f32) -> (u128, i32) {
        let (ml, el) = decompose(lo);
        let (mh, eh) = decompose(hi);
        let e = el.min(eh);
        // The midpoint is (ml 2^(el - e) + mh 2^(eh - e)) 2^(e - 1).
        let sum = (ml << (el - e) as u32) + (mh << (eh - e) as u32);
        (sum * sum * sum, 3 * (e - 1))
    }

    /// Whether the positive finite `y` is the `f32` nearest to the cube root of the positive finite
    /// `x`, decided in exact integer arithmetic: the root lies between the midpoints to the two
    /// neighbours of `y` exactly when `x` lies between their cubes. A midpoint cannot be a cube
    /// root of an `f32` (its cube has more than 24 significant bits), so there are no ties.
    fn is_correctly_rounded(x: f32, y: f32) -> bool {
        let below = f32::from_bits(y.to_bits() - 1);
        let above = f32::from_bits(y.to_bits() + 1);
        let (mx, ex) = decompose(x);
        let (low, e_low) = midpoint_cubed(below, y);
        let (high, e_high) = midpoint_cubed(y, above);
        less(low, e_low, mx, ex) && less(mx, ex, high, e_high)
    }

    /// The cube root of `x` in `f64` by Newton's method from a rough start, a different
    /// algorithm from the one under test (division by `t^2`, converged to the last bit of `f64`).
    fn newton(x: f64) -> f64 {
        let mut t = f64::from(f32::from_bits((x as f32).to_bits() / 3 + 0x2A50_0000));
        for _ in 0..40 {
            let next = (2.0 * t + x / (t * t)) / 3.0;
            if next == t {
                break;
            }
            t = next;
        }
        t
    }

    /// One unit in the last place of an `f32` near the `f64` value `c` (exact, from the exponent).
    fn ulp_at(c: f64) -> f64 {
        let exponent = ((c.to_bits() >> 52) & 0x7FF) as i32 - 1023;
        f64::from_bits(((exponent - 23 + 1023) as u64) << 52)
    }

    /// The error of `y` against the cube root of `x`, in ulps of the root.
    fn error_ulps(x: f32, y: f32) -> f64 {
        let c = newton(f64::from(x));
        (f64::from(y) - c).abs() / ulp_at(c)
    }

    // ---- fixed bits -------------------------------------------------------------------------

    /// Inputs and the expected bits of the result: subnormals, the smallest normal, one, eight,
    /// 0.18, the largest float, negatives and exact cubes. Each expected value was checked with
    /// `is_correctly_rounded`, which does not use any cube root.
    const FIXED: [(u32, u32); 26] = [
        (0x0000_0001, 0x26A1_4518), // the smallest subnormal, 1.4e-45
        (0x0000_0400, 0x284B_2FF5), // 1.4e-42
        (0x0040_0000, 0x2A4B_2FF5), // the middle of the subnormals
        (0x007F_FFFF, 0x2A7F_FFFF), // the largest subnormal
        (0x0080_0000, 0x2A80_0000), // the smallest normal
        (0x0080_0001, 0x2A80_0000),
        (0x3F80_0000, 0x3F80_0000), // 1
        (0x4000_0000, 0x3FA1_4518), // 2
        (0x4100_0000, 0x4000_0000), // 8
        (0x3E38_51EC, 0x3F10_8B0B), // 0.18
        (0x3F00_0000, 0x3F4B_2FF5), // 0.5
        (0x4120_0000, 0x4009_E242), // 10
        (0x4049_0FDB, 0x3FBB_77BF), // pi
        (0x4003_53B5, 0x3FA2_A7BC), // the input closest to a midpoint of two roots, 2^-49.07
        (0x4052_6132, 0x3FBE_51F0), // the first input a slightly low result gets wrong
        (0x41D8_0000, 0x4040_0000), // 27
        (0x447A_0000, 0x4120_0000), // 1000
        (0x3E00_0000, 0x3F00_0000), // 0.125
        (0x7F7F_FFFF, 0x54CB_2FF5), // f32::MAX
        (0xBF80_0000, 0xBF80_0000), // -1
        (0xC100_0000, 0xC000_0000), // -8
        (0xBE38_51EC, 0xBF10_8B0B), // -0.18
        (0xC1D8_0000, 0xC040_0000), // -27
        (0xFF7F_FFFF, 0xD4CB_2FF5), // -f32::MAX
        (0x8000_0001, 0xA6A1_4518), // the smallest negative subnormal
        (0x8080_0000, 0xAA80_0000), // minus the smallest normal
    ];

    #[test]
    fn fixed_inputs_give_the_expected_bits() {
        for (input, expected) in FIXED {
            let got = cbrt(f32::from_bits(input)).to_bits();
            assert_eq!(
                got,
                expected,
                "cbrt({:e}) = {:#010X}, expected {expected:#010X}",
                f32::from_bits(input),
                got
            );
        }
    }

    #[test]
    fn the_expected_bits_are_the_correctly_rounded_roots() {
        for (input, expected) in FIXED {
            let (x, y) = (
                f32::from_bits(input & ABS_MASK),
                f32::from_bits(expected & ABS_MASK),
            );
            assert!(is_correctly_rounded(x, y), "{x:e} -> {y:e}");
        }
    }

    // ---- the reference itself ----------------------------------------------------------------

    #[test]
    fn the_exact_check_accepts_the_right_answer_and_rejects_its_neighbours() {
        for bits in [
            0x3F80_0001_u32,
            0x4049_0FDB,
            0x3E38_51EC,
            0x0000_0123,
            0x7F00_0000,
        ] {
            let x = f32::from_bits(bits);
            let r = cbrt(x);
            assert!(is_correctly_rounded(x, r), "{x:e}");
            assert!(!is_correctly_rounded(x, f32::from_bits(r.to_bits() + 1)));
            assert!(!is_correctly_rounded(x, f32::from_bits(r.to_bits() - 1)));
        }
    }

    #[test]
    fn newton_and_the_exact_check_agree_on_the_error() {
        // The error of a correctly rounded result is at most half an ulp.
        for bits in (ONE_BITS..EIGHT_BITS).step_by(104_729) {
            let x = f32::from_bits(bits);
            let err = error_ulps(x, cbrt(x));
            assert!(err <= 0.500_001, "{x:e}: {err}");
        }
    }

    // ---- error bound -------------------------------------------------------------------------

    #[test]
    fn the_result_is_correctly_rounded_over_the_whole_range() {
        // Every 4099th float of the positive finite range, about 520 000 of them, every
        // exponent included.
        let mut bits = 1;
        let mut worst = 0.0_f64;
        while bits <= MAX_FINITE_BITS {
            let x = f32::from_bits(bits);
            let y = cbrt(x);
            assert!(is_correctly_rounded(x, y), "{x:e} ({bits:#010X}) -> {y:e}");
            if bits % 16 == 0 {
                worst = worst.max(error_ulps(x, y));
            }
            bits += 4099;
        }
        assert!(worst <= 0.500_001, "{worst}");
    }

    #[test]
    fn the_subnormals_are_correctly_rounded() {
        for bits in (1..MIN_NORMAL_BITS).step_by(1009) {
            let x = f32::from_bits(bits);
            assert!(is_correctly_rounded(x, cbrt(x)), "{x:e}");
        }
        // The first thousand one by one.
        for bits in 1..1000 {
            let x = f32::from_bits(bits);
            assert!(is_correctly_rounded(x, cbrt(x)), "{x:e}");
        }
    }

    /// 2^e exactly, for `e` from -149 to 127 (below -126 the result is subnormal).
    fn power_of_two(e: i32) -> f32 {
        if e >= -126 {
            f32::from_bits(((e + 127) as u32) << 23)
        } else {
            f32::from_bits(1 << (e + 149))
        }
    }

    #[test]
    fn exact_cubes_give_the_exact_root() {
        // A cube that an f32 holds exactly (at most 24 significant bits) has an exact root.
        let mut seen = 0;
        for n in 1_u64..=4096 {
            let cube = n * n * n;
            let significant = 64 - cube.leading_zeros() - cube.trailing_zeros();
            if significant <= 24 {
                assert_eq!(cbrt(cube as f32), n as f32, "{n}");
                assert_eq!(cbrt(-(cube as f32)), -(n as f32), "{n}");
                seen += 1;
            }
        }
        assert!(seen > 300, "{seen}");
        // Powers of eight, from the subnormals to the top of the range.
        for k in -49..=42 {
            assert_eq!(cbrt(power_of_two(3 * k)), power_of_two(k), "2^{}", 3 * k);
        }
    }

    // ---- scale, symmetry, order ---------------------------------------------------------------

    #[test]
    fn scaling_by_eight_scales_the_root_by_two_bit_for_bit() {
        // The claim that [1, 8) stands for all normals: x and 8x have roots in the ratio 2 exactly,
        // as long as both are normal and finite.
        for bits in (MIN_NORMAL_BITS..MAX_FINITE_BITS / 2).step_by(7919) {
            let x = f32::from_bits(bits);
            let x8 = x * 8.0;
            if x8.is_finite() {
                assert_eq!(cbrt(x8), cbrt(x) * 2.0, "{x:e}");
            }
        }
        // Subnormals: 8^8 x is the scaled value the function uses.
        for bits in (1..MIN_NORMAL_BITS).step_by(997) {
            let x = f32::from_bits(bits);
            assert_eq!(cbrt(x), cbrt(x * SUBNORMAL_SCALE) / 256.0, "{x:e}");
        }
    }

    #[test]
    fn the_root_is_odd() {
        for bits in (1..=MAX_FINITE_BITS).step_by(15_731) {
            let x = f32::from_bits(bits);
            assert_eq!(cbrt(-x).to_bits(), (-cbrt(x)).to_bits(), "{x:e}");
        }
    }

    #[test]
    fn the_root_never_decreases() {
        // Runs of consecutive floats around the places where the formula could break: the
        // subnormal border, powers of eight, one, a cube, the top of the range.
        for start in [
            0x0000_0001_u32,
            0x007F_FF00,
            0x3F7F_FF00,
            0x4100_0000 - 5000,
            0x41D8_0000 - 5000,
            0x7F7F_0000,
            0x2A55_5555,
        ] {
            let mut previous = cbrt(f32::from_bits(start));
            for bits in start + 1..(start + 10_000).min(MAX_FINITE_BITS) {
                let y = cbrt(f32::from_bits(bits));
                assert!(y >= previous, "{bits:#010X}");
                if bits >= MIN_NORMAL_BITS {
                    // Neighbouring inputs are 2^-23 apart in relative terms, the roots 2^-23 / 3.
                    assert!(y.to_bits() - previous.to_bits() <= 1, "{bits:#010X}");
                }
                previous = y;
            }
        }
        // And a coarse walk over the whole range.
        let mut previous = 0.0_f32;
        for bits in (0..=MAX_FINITE_BITS).step_by(65_537) {
            let y = cbrt(f32::from_bits(bits));
            assert!(y >= previous, "{bits:#010X}");
            previous = y;
        }
    }

    // ---- edge values --------------------------------------------------------------------------

    #[test]
    fn zeros_keep_their_sign() {
        assert_eq!(cbrt(0.0).to_bits(), 0.0_f32.to_bits());
        assert_eq!(cbrt(-0.0).to_bits(), (-0.0_f32).to_bits());
    }

    #[test]
    fn infinities_and_nan_pass_through() {
        assert_eq!(cbrt(f32::INFINITY), f32::INFINITY);
        assert_eq!(cbrt(f32::NEG_INFINITY), f32::NEG_INFINITY);
        assert!(cbrt(f32::NAN).is_nan());
        assert!(cbrt(-f32::NAN).is_nan());
        assert!(cbrt(f32::from_bits(0x7FC0_0001)).is_nan());
        assert!(cbrt(f32::from_bits(0x7F80_0001)).is_nan());
    }

    #[test]
    fn the_largest_and_smallest_values_stay_finite_and_non_zero() {
        let top = cbrt(f32::MAX);
        assert!(top.is_finite() && top > 6.9e12 && top < 7.0e12, "{top}");
        let bottom = cbrt(f32::from_bits(1));
        assert!(bottom > 1.0e-15 && bottom < 1.2e-15, "{bottom}");
    }

    #[test]
    fn the_seed_is_within_four_percent_everywhere() {
        // What step 3 of the module docs relies on.
        let mut worst = 0.0_f64;
        for bits in (ONE_BITS..EIGHT_BITS).step_by(251) {
            let x = f32::from_bits(bits);
            let seed = f64::from(f32::from_bits(bits / 3 + CBRT_SEED_OFFSET));
            worst = worst.max((seed / newton(f64::from(x)) - 1.0).abs());
        }
        assert!(worst < 0.04, "{worst}");
    }

    // ---- ignored: exhaustive, timing, the library, the seed search ---------------------------

    #[test]
    #[ignore = "exhaustive: every float in [1, 8); run with --release --ignored --nocapture"]
    fn every_float_in_one_to_eight_is_correctly_rounded() {
        let mut wrong = Vec::new();
        let mut worst = 0.0_f64;
        let mut sum = 0.0_f64;
        for bits in ONE_BITS..EIGHT_BITS {
            let x = f32::from_bits(bits);
            let y = cbrt(x);
            if !is_correctly_rounded(x, y) {
                wrong.push(bits);
            }
            let e = error_ulps(x, y);
            worst = worst.max(e);
            sum += e;
        }
        let count = f64::from(EIGHT_BITS - ONE_BITS);
        eprintln!(
            "ours over [1, 8): {count} floats, largest error {worst:.6} ulp, mean {:.6} ulp, \
             {} not correctly rounded",
            sum / count,
            wrong.len()
        );
        assert!(
            wrong.is_empty(),
            "first failures: {:#010X?}",
            &wrong[..wrong.len().min(8)]
        );
    }

    /// Candidate cube roots of the decision, for the figures only.
    mod candidates {
        use super::super::{halley, root, CBRT_SEED_OFFSET};

        /// Reciprocal seed offset of the division-free candidate (found by the search).
        pub const RECIPROCAL_OFFSET: u32 = 0x54A2_3279;

        /// Two Halley steps in `f32` only, in the correction form `t + t (a - t^3) / (a + 2 t^3)`,
        /// which rounds less than the plain form.
        pub fn halley_f32(x: f32) -> f32 {
            let a = x;
            let mut t = f32::from_bits(a.to_bits() / 3 + CBRT_SEED_OFFSET);
            for _ in 0..2 {
                let t3 = t * t * t;
                t += t * (a - t3) / (a + 2.0 * t3);
            }
            t
        }

        /// Reciprocal root by Newton (no division but by 3, a multiplication), then `a r^2`.
        pub fn division_free(x: f32) -> f32 {
            let a = f64::from(x);
            let mut r = f64::from(f32::from_bits(RECIPROCAL_OFFSET - x.to_bits() / 3));
            for _ in 0..3 {
                r = r * (4.0 - a * r * r * r) * (1.0 / 3.0);
            }
            (a * r * r) as f32
        }

        /// The first Halley step in `f32`, the second in `f64`.
        pub fn mixed(x: f32) -> f32 {
            let a = x;
            let t = f32::from_bits(a.to_bits() / 3 + CBRT_SEED_OFFSET);
            let t3 = t * t * t;
            let t = t * (2.0 * a + t3) / (a + 2.0 * t3);
            halley(f64::from(t), f64::from(a)) as f32
        }

        /// Ours without the sign and edge handling, for a fair timing against the others.
        pub fn ours(x: f32) -> f32 {
            root(x) as f32
        }
    }

    /// A named candidate.
    type Candidate = (&'static str, fn(f32) -> f32);

    /// The candidates and the library, by name.
    fn all_candidates() -> [Candidate; 5] {
        [
            ("library", |x: f32| x.cbrt()),
            ("B: two Halley steps in f64 (ours)", candidates::ours),
            ("C: Halley in f32 only", candidates::halley_f32),
            ("D: division-free", candidates::division_free),
            ("E: mixed f32 then f64", candidates::mixed),
        ]
    }

    #[test]
    #[ignore = "measurement: error of each candidate over [1, 8); needs --release --ignored --nocapture"]
    fn error_of_each_candidate_over_one_to_eight() {
        for (name, function) in all_candidates() {
            let (mut worst, mut sum, mut wrong) = (0.0_f64, 0.0_f64, 0_u32);
            for bits in ONE_BITS..EIGHT_BITS {
                let x = f32::from_bits(bits);
                let y = function(x);
                let e = error_ulps(x, y);
                worst = worst.max(e);
                sum += e;
                if !is_correctly_rounded(x, y) {
                    wrong += 1;
                }
            }
            let count = f64::from(EIGHT_BITS - ONE_BITS);
            eprintln!(
                "{name}: largest {worst:.4} ulp, mean {:.4} ulp, {:.2} % not correctly rounded",
                sum / count,
                100.0 * f64::from(wrong) / count
            );
        }
    }

    #[test]
    #[ignore = "measurement: time per value; needs --release --ignored --nocapture"]
    fn time_of_each_candidate() {
        use std::hint::black_box;
        use std::time::Instant;
        const N: usize = 1 << 22;
        let values: Vec<f32> = (0..N)
            .map(|i| f32::from_bits(ONE_BITS + ((i as u64 * 6 * (1 << 20)) / N as u64) as u32 * 4))
            .collect();
        for (name, function) in all_candidates() {
            let mut best = f64::MAX;
            let mut sink = 0.0_f32;
            for _ in 0..7 {
                let start = Instant::now();
                let mut sum = 0.0_f32;
                for &v in black_box(&values) {
                    sum += function(v);
                }
                sink += black_box(sum);
                best = best.min(start.elapsed().as_secs_f64());
            }
            eprintln!(
                "{name}: {:.2} ns per value (sum {sink:e})",
                best * 1e9 / N as f64
            );
        }
        // `cbrt` itself, with its edge handling.
        let mut best = f64::MAX;
        let mut sink = 0.0_f32;
        for _ in 0..7 {
            let start = Instant::now();
            let mut sum = 0.0_f32;
            for &v in black_box(&values) {
                sum += cbrt(v);
            }
            sink += black_box(sum);
            best = best.min(start.elapsed().as_secs_f64());
        }
        eprintln!(
            "cbrt() with its edge handling: {:.2} ns per value (sum {sink:e})",
            best * 1e9 / N as f64
        );
    }

    #[test]
    #[ignore = "measurement: the library over [1, 8); GTP_CBRT_LIBRARY_DIR writes the differences"]
    fn how_the_library_differs_from_ours() {
        let mut worst = 0.0_f64;
        let mut sum = 0.0_f64;
        let mut wrong = 0_u64;
        let mut differences = Vec::with_capacity((EIGHT_BITS - ONE_BITS) as usize);
        let mut largest_difference = 0_i64;
        for bits in ONE_BITS..EIGHT_BITS {
            let x = f32::from_bits(bits);
            let library = x.cbrt();
            let ours = cbrt(x);
            let e = error_ulps(x, library);
            worst = worst.max(e);
            sum += e;
            if !is_correctly_rounded(x, library) {
                wrong += 1;
            }
            let difference = i64::from(library.to_bits()) - i64::from(ours.to_bits());
            largest_difference = largest_difference.max(difference.abs());
            differences.push(difference.clamp(-128, 127) as i8 as u8);
        }
        let count = f64::from(EIGHT_BITS - ONE_BITS);
        eprintln!(
            "library over [1, 8) on {}-{}: largest error {worst:.4} ulp, mean {:.4} ulp, \
             {:.2} % not correctly rounded, largest bit difference to ours {largest_difference}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            sum / count,
            100.0 * wrong as f64 / count
        );
        if let Some(dir) = std::env::var_os("GTP_CBRT_LIBRARY_DIR") {
            let path = std::path::Path::new(&dir).join(format!(
                "{}-{}.bin",
                std::env::consts::OS,
                std::env::consts::ARCH
            ));
            std::fs::write(&path, &differences).unwrap();
            eprintln!("wrote {} bytes to {}", differences.len(), path.display());
        }
    }

    /// The floats of `[1, 8)` the seed search looks at (every 4099th) with their exact roots, or
    /// whatever `target` makes of them.
    fn seed_samples(target: impl Fn(f64) -> f64) -> Vec<(u32, f64)> {
        (ONE_BITS..EIGHT_BITS)
            .step_by(4099)
            .map(|bits| (bits, target(f64::from(f32::from_bits(bits)))))
            .collect()
    }

    /// The largest relative error of a seed over the samples.
    fn seed_error(samples: &[(u32, f64)], seed: impl Fn(u32) -> f64) -> f64 {
        samples
            .iter()
            .map(|&(bits, exact)| (seed(bits) / exact - 1.0).abs())
            .fold(0.0, f64::max)
    }

    /// The offset with the smallest largest seed error: a coarse scan of `span` either side of
    /// `guess` in steps of `coarse`, then a scan of every offset near the best.
    fn best_offset(guess: u32, span: u32, coarse: u32, error: impl Fn(u32) -> f64) -> (u32, f64) {
        let mut best = (guess, f64::MAX);
        let mut k = guess - span;
        while k <= guess + span {
            let e = error(k);
            if e < best.1 {
                best = (k, e);
            }
            k += coarse;
        }
        for k in best.0 - coarse..=best.0 + coarse {
            let e = error(k);
            if e < best.1 {
                best = (k, e);
            }
        }
        best
    }

    #[test]
    #[ignore = "derivation of CBRT_SEED_OFFSET: prints the best offset; run with --ignored --nocapture"]
    fn search_the_seed_offset() {
        // Guess: 2/3 of 127 2^23 (see the module docs).
        let guess = (2 * 127 * (1_u64 << 23) / 3) as u32;
        let roots = seed_samples(newton);
        let (k, e) = best_offset(guess, 1 << 22, 1 << 8, |k| {
            seed_error(&roots, |bits| f64::from(f32::from_bits(bits / 3 + k)))
        });
        eprintln!(
            "cube root seed: guess {guess:#010X}, best {k:#010X}, largest relative error {e:.5}; the constant in the file is {CBRT_SEED_OFFSET:#010X}"
        );
        // The reciprocal seed of the division-free candidate: bits of a^(-1/3) are
        // 4/3 127 2^23 - bits / 3.
        let guess = (4 * 127 * (1_u64 << 23) / 3) as u32;
        let reciprocals = seed_samples(|x| 1.0 / newton(x));
        let (kr, er) = best_offset(guess, 1 << 22, 1 << 8, |k| {
            seed_error(&reciprocals, |bits| f64::from(f32::from_bits(k - bits / 3)))
        });
        eprintln!(
            "reciprocal seed: guess {guess:#010X}, best {kr:#010X}, largest relative error {er:.5}; the constant in the file is {:#010X}",
            candidates::RECIPROCAL_OFFSET
        );
        assert_eq!(k, CBRT_SEED_OFFSET);
        assert_eq!(kr, candidates::RECIPROCAL_OFFSET);
    }
}
