//! Export colour accuracy (P-04): reference patches converted by `gtp_io::to_output` land within
//! Î”E2000 1 of their known values, checked with colour maths independent of Little CMS.

use gtp_benchmark::delta_e2000;
use gtp_core::recipe::OutputSpace;
use gtp_core::{AppError, ImageF32};
use gtp_io::to_output;

type Matrix = [[f64; 3]; 3];

/// The 24 ColorChecker patches as 8-bit sRGB (BabelColor averages, rounded).
const COLORCHECKER_SRGB8: [[u8; 3]; 24] = [
    [115, 82, 68],
    [194, 150, 130],
    [98, 122, 157],
    [87, 108, 67],
    [133, 128, 177],
    [103, 189, 170],
    [214, 126, 44],
    [80, 91, 166],
    [193, 90, 99],
    [94, 60, 108],
    [157, 188, 64],
    [224, 163, 46],
    [56, 61, 150],
    [70, 148, 73],
    [175, 54, 60],
    [231, 199, 31],
    [187, 86, 149],
    [8, 133, 161],
    [243, 243, 242],
    [200, 200, 200],
    [160, 160, 160],
    [122, 122, 121],
    [85, 85, 85],
    [52, 52, 52],
];

/// Linear sRGB to linear Rec.2020 (ITU-R BT.2087).
const SRGB_TO_REC2020: Matrix = [
    [0.627_404, 0.329_283, 0.043_313],
    [0.069_097, 0.919_540, 0.011_362],
    [0.016_391, 0.088_013, 0.895_595],
];
const REC2020_TO_XYZ: Matrix = [
    [0.636_958, 0.144_617, 0.168_881],
    [0.262_700, 0.677_998, 0.059_302],
    [0.000_000, 0.028_073, 1.060_985],
];
const SRGB_TO_XYZ: Matrix = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175_0],
    [0.019_333_9, 0.119_192_0, 0.950_304_1],
];
const ADOBE_TO_XYZ: Matrix = [
    [0.576_730_9, 0.185_554_0, 0.188_185_2],
    [0.297_376_9, 0.627_349_1, 0.075_274_1],
    [0.027_034_3, 0.070_687_2, 0.991_108_5],
];
const PROPHOTO_TO_XYZ_D50: Matrix = [
    [0.797_674_9, 0.135_191_7, 0.031_353_4],
    [0.288_040_2, 0.711_874_1, 0.000_085_7],
    [0.000_000_0, 0.000_000_0, 0.825_210_0],
];
/// Bradford chromatic adaptation from D65 to D50.
const BRADFORD_D65_TO_D50: Matrix = [
    [1.047_811_2, 0.022_886_6, -0.050_127_0],
    [0.029_542_4, 0.990_484_4, -0.017_049_1],
    [-0.009_234_5, 0.015_043_6, 0.752_131_6],
];

fn mul(m: &Matrix, v: [f64; 3]) -> [f64; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// CIELAB of `xyz` relative to the white of `to_xyz` (its RGB 1, 1, 1).
fn lab(xyz: [f64; 3], white: [f64; 3]) -> [f32; 3] {
    let f = |t: f64| {
        const D: f64 = 6.0 / 29.0;
        if t > D * D * D {
            t.cbrt()
        } else {
            t / (3.0 * D * D) + 4.0 / 29.0
        }
    };
    let [fx, fy, fz] = [0, 1, 2].map(|i| f(xyz[i] / white[i]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)].map(|v| v as f32)
}

fn white_of(m: &Matrix) -> [f64; 3] {
    mul(m, [1.0; 3])
}

/// The patches in linear Rec.2020, as the pipeline would hand them to the export.
fn patches() -> Vec<[f64; 3]> {
    COLORCHECKER_SRGB8
        .iter()
        .map(|p| {
            mul(
                &SRGB_TO_REC2020,
                p.map(|v| srgb_decode(f64::from(v) / 255.0)),
            )
        })
        .collect()
}

fn export(space: OutputSpace) -> Result<Vec<[f64; 3]>, AppError> {
    let data: Vec<f32> = patches().iter().flatten().map(|v| *v as f32).collect();
    let img = ImageF32::new(24, 1, 3, data)?;
    let out = to_output(&img, space)?;
    let (pixels, _) = out.data.as_chunks::<3>();
    Ok(pixels
        .iter()
        .map(|p| p.map(|v| f64::from(v) / 65535.0))
        .collect())
}

/// The reference Lab of each patch: from Rec.2020 (D65), adapted to D50 when `d50`.
fn reference_labs(d50: bool) -> Vec<[f32; 3]> {
    let adapt = |xyz: [f64; 3]| {
        if d50 {
            mul(&BRADFORD_D65_TO_D50, xyz)
        } else {
            xyz
        }
    };
    let white = adapt(white_of(&REC2020_TO_XYZ));
    patches()
        .into_iter()
        .map(|rgb| lab(adapt(mul(&REC2020_TO_XYZ, rgb)), white))
        .collect()
}

fn assert_within_one(space: OutputSpace, got: &[[f32; 3]], want: &[[f32; 3]]) {
    let worst = got
        .iter()
        .zip(want)
        .map(|(g, w)| delta_e2000(*g, *w))
        .fold(0.0f32, f32::max);
    assert!(worst < 1.0, "{space:?}: worst patch Î”E2000 {worst}");
}

#[test]
fn srgb_export_returns_the_colorchecker_values() {
    let out = export(OutputSpace::Srgb).unwrap();
    for (got, want) in out.iter().zip(COLORCHECKER_SRGB8) {
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g * 255.0 - f64::from(w)).abs() < 0.6,
                "{got:?} vs {want:?}"
            );
        }
    }
    let labs: Vec<_> = out
        .iter()
        .map(|p| {
            lab(
                mul(&SRGB_TO_XYZ, p.map(srgb_decode)),
                white_of(&SRGB_TO_XYZ),
            )
        })
        .collect();
    assert_within_one(OutputSpace::Srgb, &labs, &reference_labs(false));
}

#[test]
fn adobe_rgb_export_is_within_one_delta_e() {
    let labs: Vec<_> = export(OutputSpace::AdobeRgb)
        .unwrap()
        .iter()
        .map(|p| {
            let linear = p.map(|v| v.powf(563.0 / 256.0));
            lab(mul(&ADOBE_TO_XYZ, linear), white_of(&ADOBE_TO_XYZ))
        })
        .collect();
    assert_within_one(OutputSpace::AdobeRgb, &labs, &reference_labs(false));
}

#[test]
fn prophoto_export_is_within_one_delta_e_after_adaptation_to_d50() {
    let labs: Vec<_> = export(OutputSpace::Prophoto)
        .unwrap()
        .iter()
        .map(|p| {
            let linear = p.map(|v| v.powf(1.8));
            lab(
                mul(&PROPHOTO_TO_XYZ_D50, linear),
                white_of(&PROPHOTO_TO_XYZ_D50),
            )
        })
        .collect();
    assert_within_one(OutputSpace::Prophoto, &labs, &reference_labs(true));
}
