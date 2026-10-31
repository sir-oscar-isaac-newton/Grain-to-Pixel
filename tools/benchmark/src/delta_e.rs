//! Colour difference: linear Rec.2020 to CIELAB, and CIEDE2000.
//!
//! CIEDE2000 follows G. Sharma, W. Wu and E. N. Dalal, "The CIEDE2000 color-difference formula:
//! implementation notes, supplementary test data, and mathematical observations", Color Research
//! and Application 30(1), 2005, with kL = kC = kH = 1. It is computed in double precision.

/// Linear Rec.2020 RGB to CIE XYZ, D65 white (ITU-R BT.2020 primaries).
const REC2020_TO_XYZ: [[f64; 3]; 3] = [
    [0.636_958, 0.144_617, 0.168_881],
    [0.262_700, 0.677_998, 0.059_302],
    [0.000_000, 0.028_073, 1.060_985],
];

/// CIELAB of a linear Rec.2020 colour, relative to the Rec.2020 white (RGB 1, 1, 1), so that white
/// is exactly L* 100, a* 0, b* 0. Values above 1 give L* above 100; negative values stay finite.
pub fn rec2020_to_lab(rgb: [f32; 3]) -> [f32; 3] {
    let rgb = rgb.map(f64::from);
    let xyz = REC2020_TO_XYZ.map(|row| row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2]);
    let white = REC2020_TO_XYZ.map(|row| row[0] + row[1] + row[2]);
    let [fx, fy, fz] = [0, 1, 2].map(|i| lab_f(xyz[i] / white[i]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)].map(|v| v as f32)
}

fn lab_f(t: f64) -> f64 {
    const DELTA: f64 = 6.0 / 29.0;
    if t > DELTA * DELTA * DELTA {
        t.cbrt()
    } else {
        t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
    }
}

/// The CIEDE2000 difference between two CIELAB colours. About 1 is a just-noticeable difference.
pub fn delta_e2000(lab1: [f32; 3], lab2: [f32; 3]) -> f32 {
    let [l1, a1, b1] = lab1.map(f64::from);
    let [l2, a2, b2] = lab2.map(f64::from);
    let pow25_7 = 25f64.powi(7);

    let c_mean = (a1.hypot(b1) + a2.hypot(b2)) / 2.0;
    let g = 0.5 * (1.0 - (c_mean.powi(7) / (c_mean.powi(7) + pow25_7)).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = (a1p.hypot(b1), a2p.hypot(b2));
    let hue = |b: f64, a: f64| {
        if a == 0.0 && b == 0.0 {
            0.0
        } else {
            b.atan2(a).to_degrees().rem_euclid(360.0)
        }
    };
    let (h1p, h2p) = (hue(b1, a1p), hue(b2, a2p));
    let chroma_product = c1p * c2p;

    let dl = l2 - l1;
    let dc = c2p - c1p;
    let dh_angle = if chroma_product == 0.0 {
        0.0
    } else {
        let d = h2p - h1p;
        if d > 180.0 {
            d - 360.0
        } else if d < -180.0 {
            d + 360.0
        } else {
            d
        }
    };
    let dh = 2.0 * chroma_product.sqrt() * (dh_angle.to_radians() / 2.0).sin();

    let l_mean = (l1 + l2) / 2.0;
    let c_mean_p = (c1p + c2p) / 2.0;
    let h_mean = if chroma_product == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };

    let cos_deg = |deg: f64| deg.to_radians().cos();
    let t = 1.0 - 0.17 * cos_deg(h_mean - 30.0)
        + 0.24 * cos_deg(2.0 * h_mean)
        + 0.32 * cos_deg(3.0 * h_mean + 6.0)
        - 0.20 * cos_deg(4.0 * h_mean - 63.0);
    let d_theta = 30.0 * (-((h_mean - 275.0) / 25.0).powi(2)).exp();
    let rc = 2.0 * (c_mean_p.powi(7) / (c_mean_p.powi(7) + pow25_7)).sqrt();
    let l50 = (l_mean - 50.0).powi(2);
    let sl = 1.0 + 0.015 * l50 / (20.0 + l50).sqrt();
    let sc = 1.0 + 0.045 * c_mean_p;
    let sh = 1.0 + 0.015 * c_mean_p * t;
    let rt = -(2.0 * d_theta).to_radians().sin() * rc;

    let (l_term, c_term, h_term) = (dl / sl, dc / sc, dh / sh);
    (l_term * l_term + c_term * c_term + h_term * h_term + rt * c_term * h_term)
        .max(0.0)
        .sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pairs from the supplementary test data of Sharma, Wu and Dalal (2005), table 1, with
    /// their published differences (rounded to 4 decimals). The numbers on the left are the
    /// table's pair numbers.
    const SHARMA: [(u32, [f32; 3], [f32; 3], f32); 16] = [
        (1, [50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
        (2, [50.0, 3.1571, -77.2803], [50.0, 0.0, -82.7485], 2.8615),
        (4, [50.0, -1.3802, -84.2814], [50.0, 0.0, -82.7485], 1.0000),
        (7, [50.0, 0.0, 0.0], [50.0, -1.0, 2.0], 2.3669),
        // 13 to 16 and 17, 19: hues on either side of 0 and 180 degrees, where the mean hue
        // and the hue difference wrap around.
        (13, [50.0, 2.49, -0.001], [50.0, -2.49, 0.0009], 7.1792),
        (14, [50.0, 2.49, -0.001], [50.0, -2.49, 0.0010], 7.1792),
        (15, [50.0, 2.49, -0.001], [50.0, -2.49, 0.0011], 7.2195),
        (16, [50.0, 2.49, -0.001], [50.0, -2.49, 0.0012], 7.2195),
        (17, [50.0, -0.001, 2.49], [50.0, 0.0009, -2.49], 4.8045),
        (19, [50.0, -0.001, 2.49], [50.0, 0.0011, -2.49], 4.7461),
        (21, [50.0, 2.5, 0.0], [73.0, 25.0, -18.0], 27.1492),
        (23, [50.0, 2.5, 0.0], [56.0, -27.0, -3.0], 31.9030),
        (25, [50.0, 2.5, 0.0], [50.0, 3.1736, 0.5854], 1.0000),
        (
            34,
            [22.7233, 20.0904, -46.6940],
            [23.0331, 14.9730, -42.5619],
            2.0373,
        ),
        (
            35,
            [36.4612, 47.8580, 18.3852],
            [36.2715, 50.5065, 21.2231],
            1.4146,
        ),
        (
            36,
            [90.8027, -2.0831, 1.4410],
            [91.1528, -1.6435, 0.0447],
            1.4441,
        ),
    ];

    #[test]
    fn matches_the_published_test_pairs() {
        for (pair, lab1, lab2, want) in SHARMA {
            let got = delta_e2000(lab1, lab2);
            assert!((got - want).abs() < 1e-4, "pair {pair}: {got} vs {want}");
        }
    }

    #[test]
    fn is_symmetric_and_zero_for_equal_colours() {
        for (pair, lab1, lab2, _) in SHARMA {
            let (ab, ba) = (delta_e2000(lab1, lab2), delta_e2000(lab2, lab1));
            assert!((ab - ba).abs() < 1e-5, "pair {pair}: {ab} vs {ba}");
            assert_eq!(delta_e2000(lab1, lab1), 0.0);
        }
    }

    #[test]
    fn white_black_and_mid_grey_have_the_expected_lab() {
        let white = rec2020_to_lab([1.0; 3]);
        assert!((white[0] - 100.0).abs() < 1e-4 && white[1].abs() < 1e-4 && white[2].abs() < 1e-4);
        assert_eq!(rec2020_to_lab([0.0; 3]), [0.0; 3]);
        // 18 % grey is L* 49.5 by the CIE definition.
        let grey = rec2020_to_lab([0.18; 3]);
        assert!((grey[0] - 49.496).abs() < 1e-2, "{grey:?}");
        assert!(grey[1].abs() < 1e-4 && grey[2].abs() < 1e-4);
    }

    #[test]
    fn primaries_land_in_the_right_quadrants() {
        let red = rec2020_to_lab([1.0, 0.0, 0.0]);
        let green = rec2020_to_lab([0.0, 1.0, 0.0]);
        let blue = rec2020_to_lab([0.0, 0.0, 1.0]);
        assert!(red[1] > 0.0 && red[2] > 0.0, "red is +a +b: {red:?}");
        assert!(
            green[1] < 0.0 && green[2] > 0.0,
            "green is -a +b: {green:?}"
        );
        assert!(blue[2] < 0.0, "blue is -b: {blue:?}");
    }

    #[test]
    fn out_of_range_values_stay_finite() {
        for rgb in [[4.0, 2.0, 0.5], [-0.1, 0.2, 0.3], [0.0, -1.0, 0.0]] {
            let lab = rec2020_to_lab(rgb);
            assert!(lab.iter().all(|v| v.is_finite()), "{rgb:?} -> {lab:?}");
        }
    }
}
