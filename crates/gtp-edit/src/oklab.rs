//! Conversions between linear Rec.2020 RGB and Oklab / Oklch, the colour model of the colour mixer
//! (docs/specification.md section 4.2, ADR 0012).
//!
//! The path is linear Rec.2020 -> CIE XYZ (D65) -> LMS (Oklab's first matrix) -> cube root -> Lab
//! (Oklab's second matrix). The first two matrices are multiplied into one ([`REC2020_TO_LMS`]) and
//! so are their inverses ([`LMS_TO_REC2020`]); the tests prove both against the published matrices.
//! The cube root is `cbrt`, which is defined for negative values and keeps the sign, so colours
//! outside the Rec.2020 gamut (negative components) convert and come back. Nothing is clamped.
//!
//! Oklch is Lab in polar form: lightness `L`, chroma `C = hypot(a, b)` and hue
//! `h = atan2(b, a)` in degrees from 0 (included) to 360 (excluded).

/// Linear Rec.2020 to CIE XYZ (D65), ITU-R BT.2020.
pub const REC2020_TO_XYZ: [[f32; 3]; 3] = [
    [0.636_958_05, 0.144_616_9, 0.168_880_98],
    [0.262_700_2, 0.677_998_07, 0.059_301_717],
    [0.0, 0.028_072_693, 1.060_985_1],
];
/// CIE XYZ to Oklab's cone responses (LMS), the first matrix of Oklab (Ottosson, 2020).
pub const XYZ_TO_LMS: [[f32; 3]; 3] = [
    [0.818_933, 0.361_866_74, -0.128_859_71],
    [0.032_984_544, 0.929_311_9, 0.036_145_64],
    [0.048_200_3, 0.264_366_27, 0.633_851_7],
];
/// Cube-rooted LMS to Lab, the second matrix of Oklab.
pub const LMS_TO_LAB: [[f32; 3]; 3] = [
    [0.210_454_26, 0.793_617_8, -0.004_072_047],
    [1.977_998_5, -2.428_592_2, 0.450_593_7],
    [0.025_904_037, 0.782_771_77, -0.808_675_77],
];
/// Lab to cube-rooted LMS, the published inverse of [`LMS_TO_LAB`].
pub const LAB_TO_LMS: [[f32; 3]; 3] = [
    [1.0, 0.396_337_78, 0.215_803_76],
    [1.0, -0.105_561_35, -0.063_854_17],
    [1.0, -0.089_484_18, -1.291_485_5],
];
/// Linear Rec.2020 straight to LMS: [`XYZ_TO_LMS`] times [`REC2020_TO_XYZ`].
pub const REC2020_TO_LMS: [[f32; 3]; 3] = [
    [0.616_688_44, 0.360_159_07, 0.023_043_294],
    [0.265_140_2, 0.635_856_5, 0.099_030_234],
    [0.100_150_645, 0.204_004_32, 0.696_324_7],
];
/// LMS straight to linear Rec.2020: the inverse of [`REC2020_TO_LMS`].
pub const LMS_TO_REC2020: [[f32; 3]; 3] = [
    [2.140_140_4, -1.246_355_9, 0.106_431_73],
    [-0.884_832_45, 2.163_172_7, -0.278_361_6],
    [-0.048_579_06, -0.454_490_9, 1.502_356_3],
];

/// Degrees in a full turn of the hue.
pub const FULL_TURN_DEG: f32 = 360.0;

fn mul(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// Linear Rec.2020 to Oklab `[L, a, b]`.
pub fn rgb_to_oklab(rgb: [f32; 3]) -> [f32; 3] {
    let lms = mul(&REC2020_TO_LMS, rgb).map(f32::cbrt);
    mul(&LMS_TO_LAB, lms)
}

/// Oklab `[L, a, b]` to linear Rec.2020.
pub fn oklab_to_rgb(lab: [f32; 3]) -> [f32; 3] {
    let lms = mul(&LAB_TO_LMS, lab).map(|c| c * c * c);
    mul(&LMS_TO_REC2020, lms)
}

/// Linear Rec.2020 to Oklch `[L, C, h]` with `h` in degrees, 0 to 360.
pub fn rgb_to_oklch(rgb: [f32; 3]) -> [f32; 3] {
    let [l, a, b] = rgb_to_oklab(rgb);
    [l, a.hypot(b), hue_deg(a, b)]
}

/// Oklch `[L, C, h]` (`h` in degrees) to linear Rec.2020.
pub fn oklch_to_rgb(lch: [f32; 3]) -> [f32; 3] {
    let (sin, cos) = lch[2].to_radians().sin_cos();
    oklab_to_rgb([lch[0], lch[1] * cos, lch[1] * sin])
}

/// The hue of the Lab pair `(a, b)` in degrees, 0 to 360 (0 for a grey).
pub fn hue_deg(a: f32, b: f32) -> f32 {
    let h = b.atan2(a).to_degrees();
    if h < 0.0 {
        // `-1e-9 + 360` rounds to exactly 360 in f32; keep the range half open.
        let wrapped = h + FULL_TURN_DEG;
        if wrapped >= FULL_TURN_DEG {
            0.0
        } else {
            wrapped
        }
    } else {
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Linear sRGB to linear Rec.2020 (ITU-R BT.2087), for the reference colours.
    const SRGB_TO_REC2020: [[f32; 3]; 3] = [
        [0.627_404, 0.329_282, 0.043_313_6],
        [0.069_097, 0.919_54, 0.011_361_2],
        [0.016_391_6, 0.088_013_2, 0.895_595],
    ];

    fn stream(seed: u32) -> impl FnMut() -> f32 {
        let mut state = seed;
        move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32
        }
    }

    fn product(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
        let mut out = [[0.0; 3]; 3];
        for (i, row) in out.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
            }
        }
        out
    }

    fn close(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3], tolerance: f32) {
        for i in 0..3 {
            for j in 0..3 {
                assert!((a[i][j] - b[i][j]).abs() < tolerance, "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn the_combined_matrix_is_the_product_of_the_published_ones() {
        close(
            &REC2020_TO_LMS,
            &product(&XYZ_TO_LMS, &REC2020_TO_XYZ),
            1e-6,
        );
    }

    #[test]
    fn the_inverse_matrices_invert() {
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        close(&product(&REC2020_TO_LMS, &LMS_TO_REC2020), &identity, 1e-6);
        close(&product(&LMS_TO_LAB, &LAB_TO_LMS), &identity, 1e-6);
    }

    #[test]
    fn the_combined_inverse_is_the_product_of_the_published_inverses() {
        // BT.2020 XYZ to RGB, and the inverse of Oklab's first matrix.
        let xyz_to_rec2020 = [
            [1.716_651, -0.355_671, -0.253_366],
            [-0.666_684, 1.616_481, 0.015_769],
            [0.017_640, -0.042_771, 0.942_103],
        ];
        let lms_to_xyz = [
            [1.227_014, -0.557_8, 0.281_256],
            [-0.040_580, 1.112_257, -0.071_677],
            [-0.076_381, -0.421_482, 1.586_163],
        ];
        close(
            &LMS_TO_REC2020,
            &product(&xyz_to_rec2020, &lms_to_xyz),
            2e-5,
        );
    }

    #[test]
    fn white_is_lightness_one_without_chroma() {
        let [l, c, _] = rgb_to_oklch([1.0, 1.0, 1.0]);
        assert!((l - 1.0).abs() < 1e-3, "{l}");
        assert!(c < 1e-3, "{c}");
        assert_eq!(rgb_to_oklch([0.0; 3]), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn colours_survive_the_round_trip() {
        let mut next = stream(11);
        let mut samples = vec![
            [0.0, 0.0, 0.0],
            [1e-6, 2e-6, 5e-7],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [-0.05, 0.4, 0.9],
            [3.0, 0.01, 0.0],
        ];
        for i in 0..2000 {
            let scale = if i % 3 == 0 { 1e-3 } else { 1.5 };
            samples.push([next() * scale, next() * scale, next() * scale]);
            // Very saturated: one channel near zero.
            let mut c = [next() * 2.0, next() * 2.0, next() * 2.0];
            c[i % 3] = next() * 1e-4;
            samples.push(c);
        }
        for rgb in samples {
            let lab = oklab_to_rgb(rgb_to_oklab(rgb));
            let lch = oklch_to_rgb(rgb_to_oklch(rgb));
            for k in 0..3 {
                let tolerance = 1e-4 * rgb[k].abs().max(1.0);
                assert!((lab[k] - rgb[k]).abs() < tolerance, "{rgb:?} {lab:?}");
                assert!((lch[k] - rgb[k]).abs() < tolerance, "{rgb:?} {lch:?}");
            }
        }
    }

    #[test]
    fn the_hue_runs_from_zero_up_to_360() {
        for (a, b, want) in [
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 90.0),
            (-1.0, 0.0, 180.0),
            (0.0, -1.0, 270.0),
        ] {
            assert!((hue_deg(a, b) - want).abs() < 1e-4);
        }
        assert!(hue_deg(1.0, -1e-9) < FULL_TURN_DEG);
    }

    #[test]
    fn the_srgb_primaries_match_the_reference_oklch_values() {
        // Reference Oklch of the sRGB primaries (Ottosson's published values, sRGB white); they
        // differ a little because this chain starts from the Rec.2020 matrix.
        let cases = [
            ([1.0, 0.0, 0.0], [0.627_96, 0.257_68, 29.23]),
            ([0.0, 1.0, 0.0], [0.866_44, 0.294_83, 142.50]),
            ([0.0, 0.0, 1.0], [0.452_01, 0.313_21, 264.05]),
        ];
        for (srgb, want) in cases {
            let rec2020 = mul(&SRGB_TO_REC2020, srgb);
            let got = rgb_to_oklch(rec2020);
            assert!((got[0] - want[0]).abs() < 1e-3, "{got:?} {want:?}");
            assert!((got[1] - want[1]).abs() < 2e-3, "{got:?} {want:?}");
            assert!((got[2] - want[2]).abs() < 0.2, "{got:?} {want:?}");
        }
    }

    #[test]
    fn negative_components_keep_their_sign_through_the_cube_root() {
        let rgb = [-0.1, 0.5, 0.2];
        let back = oklab_to_rgb(rgb_to_oklab(rgb));
        assert!(back[0] < 0.0 && (back[0] - rgb[0]).abs() < 1e-5, "{back:?}");
    }
}
