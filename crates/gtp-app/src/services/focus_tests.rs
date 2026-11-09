#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Tests of the focus check's measurements (see `focus.rs`).

use super::*;

const SIDE: usize = 240;

/// Deterministic noise in -1 to 1 for the pixel (x, y).
fn noise(x: usize, y: usize) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// A textured plane at `mean` brightness, each pixel the box average of the noise over a radius
/// that `blur(x, y)` gives, so that a larger radius is softer.
fn plane(mean: f32, blur: impl Fn(usize, usize) -> usize) -> ImageF32 {
    let mut data = Vec::with_capacity(SIDE * SIDE);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let r = blur(x, y);
            let (mut sum, mut n) = (0.0, 0.0);
            for yy in y.saturating_sub(r)..(y + r + 1).min(SIDE) {
                for xx in x.saturating_sub(r)..(x + r + 1).min(SIDE) {
                    sum += noise(xx, yy);
                    n += 1.0;
                }
            }
            data.push(mean * (1.0 + 0.5 * sum / n));
        }
    }
    ImageF32::new(SIDE as u32, SIDE as u32, 1, data).unwrap()
}

fn scaled(image: &ImageF32, factor: f32) -> ImageF32 {
    ImageF32 {
        data: image.data.iter().map(|v| v * factor).collect(),
        ..image.clone()
    }
}

#[test]
fn a_sharp_texture_scores_higher_than_a_blurred_one() {
    let sharp = plane(0.5, |_, _| 0);
    let soft = plane(0.5, |_, _| 1);
    let softer = plane(0.5, |_, _| 3);
    let score = |p: &ImageF32| region_sharpness(&p.data, SIDE, (0, 0, SIDE, SIDE));
    assert!(
        score(&sharp) > 2.0 * score(&soft),
        "{} {}",
        score(&sharp),
        score(&soft)
    );
    assert!(score(&soft) > 2.0 * score(&softer));
}

#[test]
fn sharpness_does_not_depend_on_the_brightness() {
    let base = plane(0.4, |_, _| 0);
    let a = region_sharpness(&base.data, SIDE, (0, 0, SIDE, SIDE));
    let b = region_sharpness(&scaled(&base, 0.25).data, SIDE, (0, 0, SIDE, SIDE));
    assert!((a - b).abs() < a * 1e-3, "{a} {b}");
}

#[test]
fn a_flat_dark_or_tiny_region_counts_as_zero() {
    let even = vec![0.5f32; SIDE * SIDE];
    assert_eq!(region_sharpness(&even, SIDE, (0, 0, SIDE, SIDE)), 0.0);
    let dark = scaled(&plane(0.5, |_, _| 0), 1e-6);
    assert_eq!(region_sharpness(&dark.data, SIDE, (0, 0, SIDE, SIDE)), 0.0);
    let sharp = plane(0.5, |_, _| 0);
    assert_eq!(region_sharpness(&sharp.data, SIDE, (0, 0, 2, 2)), 0.0);
}

#[test]
fn an_even_plane_gives_a_flat_map_and_the_pattern_flat() {
    let analysis = analyse(&plane(0.5, |_, _| 0), None).unwrap();
    assert!(
        analysis.map.iter().flatten().all(|v| *v >= 0.85),
        "{:?}",
        analysis.map
    );
    assert_eq!(analysis.pattern, FocusPattern::Flat);
    assert_eq!(analysis.map, analysis.raw_map);
    let best = analysis.map.iter().flatten().copied().fold(0.0, f32::max);
    assert_eq!(best, 1.0);
}

/// The region of the 3 x 3 grid a pixel is in, as (row, column).
fn cell(x: usize, y: usize) -> (usize, usize) {
    (y * 3 / SIDE, x * 3 / SIDE)
}

#[test]
fn a_plane_blurred_more_towards_the_edges_is_bowed() {
    let p = plane(0.5, |x, y| usize::from(cell(x, y) != (1, 1)) * 2);
    let analysis = analyse(&p, None).unwrap();
    assert_eq!(analysis.pattern, FocusPattern::Bowed, "{:?}", analysis.map);
    assert_eq!(analysis.map[1][1], 1.0);
}

#[test]
fn a_plane_blurred_more_towards_one_side_is_tilted_that_way() {
    let by_column = |soft: usize| {
        plane(0.5, move |x, y| {
            let c = cell(x, y).1;
            (if soft == 0 { 2 - c } else { c }) * 2 / 2
        })
    };
    let left = analyse(&by_column(0), None).unwrap();
    assert_eq!(
        left.pattern,
        FocusPattern::Tilted {
            side: TiltSide::Left
        },
        "{:?}",
        left.map
    );
    let right = analyse(&by_column(2), None).unwrap();
    assert_eq!(
        right.pattern,
        FocusPattern::Tilted {
            side: TiltSide::Right
        },
        "{:?}",
        right.map
    );
    let top = analyse(&plane(0.5, |x, y| (2 - cell(x, y).0) * 2 / 2), None).unwrap();
    assert_eq!(
        top.pattern,
        FocusPattern::Tilted {
            side: TiltSide::Top
        },
        "{:?}",
        top.map
    );
}

#[test]
fn one_soft_corner_is_a_tilt_towards_that_corner() {
    let p = plane(0.5, |x, y| if cell(x, y) == (2, 2) { 3 } else { 0 });
    let analysis = analyse(&p, None).unwrap();
    assert_eq!(
        analysis.pattern,
        FocusPattern::Tilted {
            side: TiltSide::BottomRight
        },
        "{:?}",
        analysis.map
    );
}

#[test]
fn the_patterns_follow_the_documented_thresholds_on_synthetic_maps() {
    let flat = [[1.0, 0.9, 0.86], [0.9, 1.0, 0.9], [0.86, 0.9, 1.0]];
    assert_eq!(classify(&flat), FocusPattern::Flat);
    let almost = [[1.0, 0.9, 0.84], [0.9, 1.0, 0.9], [0.9, 0.9, 1.0]];
    assert_ne!(classify(&almost), FocusPattern::Flat);

    let bowed = [[0.7, 0.75, 0.7], [0.72, 1.0, 0.75], [0.7, 0.8, 0.7]];
    assert_eq!(classify(&bowed), FocusPattern::Bowed);
    // The centre is not the best by more than 0.05.
    let off = [[0.7, 0.75, 0.7], [0.72, 0.9, 0.75], [0.7, 1.0, 0.7]];
    assert_ne!(classify(&off), FocusPattern::Bowed);
    // The outer regions differ by more than 0.2: not bowed.
    let uneven = [[0.4, 0.75, 0.7], [0.72, 1.0, 0.75], [0.7, 0.8, 0.7]];
    assert_ne!(classify(&uneven), FocusPattern::Bowed);
    // The edges drop by less than 0.15 on average: not bowed (and not flat).
    let shallow = [[0.84, 0.92, 0.84], [0.92, 1.0, 0.92], [0.84, 0.92, 0.84]];
    assert_ne!(classify(&shallow), FocusPattern::Bowed);

    let left_soft = [[0.6, 0.8, 1.0], [0.6, 0.9, 1.0], [0.6, 0.8, 0.9]];
    assert_eq!(
        classify(&left_soft),
        FocusPattern::Tilted {
            side: TiltSide::Left
        }
    );
    let bottom_soft = [[1.0, 1.0, 1.0], [0.8, 0.9, 0.8], [0.5, 0.6, 0.5]];
    assert_eq!(
        classify(&bottom_soft),
        FocusPattern::Tilted {
            side: TiltSide::Bottom
        }
    );
    let corner = [[1.0, 0.95, 0.9], [0.95, 0.9, 0.9], [0.9, 0.9, 0.6]];
    assert_eq!(
        classify(&corner),
        FocusPattern::Tilted {
            side: TiltSide::BottomRight
        }
    );
    let top_right = [[0.9, 0.9, 0.5], [0.9, 0.9, 0.95], [1.0, 0.95, 0.9]];
    assert_eq!(
        classify(&top_right),
        FocusPattern::Tilted {
            side: TiltSide::TopRight
        }
    );

    let unclear = [[1.0, 0.5, 1.0], [0.5, 1.0, 0.5], [1.0, 0.5, 1.0]];
    assert_eq!(classify(&unclear), FocusPattern::Unclear);
    assert_eq!(classify(&[[0.0; 3]; 3]), FocusPattern::Unclear);
}

#[test]
fn a_reference_takes_out_the_lens_curvature() {
    // The lens alone is bowed; a film that is flat then shows the same map.
    let lens = [[0.7, 0.75, 0.7], [0.72, 1.0, 0.75], [0.7, 0.8, 0.7]];
    assert_eq!(classify(&relative_to_best(&lens)), FocusPattern::Bowed);
    let with_reference = against_reference(&lens, &lens);
    assert!(with_reference
        .iter()
        .flatten()
        .all(|v| (*v - 1.0).abs() < 1e-6));
    assert_eq!(classify(&with_reference), FocusPattern::Flat);
    // The same lens and a film tilted to the left shows as a tilt once the lens is divided out.
    let mut tilted = lens;
    for row in &mut tilted {
        row[0] *= 0.5;
    }
    assert_eq!(
        classify(&against_reference(&tilted, &lens)),
        FocusPattern::Tilted {
            side: TiltSide::Left
        }
    );
    // The result is relative to its best again.
    let best = against_reference(&tilted, &lens)
        .iter()
        .flatten()
        .copied()
        .fold(0.0, f32::max);
    assert!((best - 1.0).abs() < 1e-6);
}

#[test]
fn a_plane_is_measured_against_a_reference_and_the_raw_map_stays_for_saving() {
    let p = plane(0.5, |x, y| usize::from(cell(x, y) != (1, 1)) * 2);
    let raw = analyse(&p, None).unwrap();
    assert_eq!(raw.pattern, FocusPattern::Bowed);
    // The reference is that very bowl, so what is left is flat.
    let with = analyse(&p, Some(&raw.raw_map)).unwrap();
    assert_eq!(with.pattern, FocusPattern::Flat, "{:?}", with.map);
    assert_eq!(with.raw_map, raw.raw_map);
}

#[test]
fn a_reference_needs_finite_values_above_zero() {
    assert!(validate_reference(&[[1.0; 3]; 3]).is_ok());
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let mut map = [[1.0; 3]; 3];
        map[2][1] = bad;
        assert_eq!(
            validate_reference(&map).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
}

#[test]
fn the_crop_is_the_middle_512_square_or_the_whole_smaller_plane() {
    assert_eq!(crop_bounds(6016, 4016), (2752, 1752, 3264, 2264));
    assert_eq!(crop_bounds(192, 128), (0, 0, 192, 128));
    assert_eq!(crop_bounds(600, 100), (44, 0, 556, 100));
}

#[test]
fn the_crop_is_a_grey_jpeg_that_fills_the_range() {
    let analysis = analyse(&plane(0.05, |_, _| 0), None).unwrap();
    assert_eq!(analysis.crop_size, (SIDE as u32, SIDE as u32));
    let mut decoder = jpeg_decoder::Decoder::new(analysis.crop.as_slice());
    let pixels = decoder.decode().unwrap();
    let info = decoder.info().unwrap();
    assert_eq!(
        (usize::from(info.width), usize::from(info.height)),
        (SIDE, SIDE)
    );
    let grey: Vec<u8> = pixels.chunks(3).map(|p| p[0]).collect();
    assert!(
        grey.iter().copied().max().unwrap() > 230,
        "a dark exposure is stretched"
    );
    assert!(pixels
        .chunks(3)
        .all(|p| p[0].abs_diff(p[1]) < 3 && p[0].abs_diff(p[2]) < 3));
}

#[test]
fn a_black_plane_gives_a_black_crop_and_no_pattern_to_trust() {
    let black = ImageF32::new(64, 64, 1, vec![0.0; 64 * 64]).unwrap();
    let analysis = analyse(&black, None).unwrap();
    assert_eq!(analysis.sharpness, 0.0);
    assert_eq!(analysis.pattern, FocusPattern::Unclear);
    assert!(jpeg_decoder::Decoder::new(analysis.crop.as_slice())
        .decode()
        .is_ok());
}

#[test]
fn base64_matches_the_standard_vectors() {
    for (input, expected) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64(input.as_bytes()), expected);
    }
}

#[test]
fn a_24_megapixel_plane_is_measured_in_about_a_second() {
    let (w, h) = (6016usize, 4016usize);
    let data: Vec<f32> = (0..w * h)
        .map(|i| 0.5 + 0.1 * noise(i % w, i / w))
        .collect();
    let plane = ImageF32::new(w as u32, h as u32, 1, data).unwrap();
    let start = std::time::Instant::now();
    let analysis = analyse(&plane, None).unwrap();
    // A loose bound (debug builds are slow); the budget is a second or two in release.
    assert!(start.elapsed() < std::time::Duration::from_secs(30));
    assert_eq!(analysis.crop_size, (512, 512));
}
