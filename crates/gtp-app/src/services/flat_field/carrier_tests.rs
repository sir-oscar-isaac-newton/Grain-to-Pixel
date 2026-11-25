//! Tests of the flat field that leaves the film carrier out (P-35, decision 0021): the same result
//! as the plain method when there is no carrier, an even picture up to the edge of the opening when
//! there is one, and the fill of the cells the carrier covers.

use gtp_core::{ErrorCode, FlatField, ImageF32};

use super::reference::build_flat_field_reference;
use super::*;

fn sources() -> Vec<String> {
    vec!["r.NEF".into(), "g.NEF".into(), "b.NEF".into()]
}

/// The light of a lamp that falls off towards the edges, differently for each colour: `k` is how
/// much each channel loses at the corners (so the corners keep `1 - k`).
fn light_at(x: usize, y: usize, (w, h): (usize, usize), k: [f32; 3]) -> [f32; 3] {
    let u = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
    let v = (y as f32 + 0.5) / h as f32 * 2.0 - 1.0;
    let r2 = (u * u + v * v) / 2.0;
    k.map(|k| 1.0 - k * r2)
}

/// A bare light with no carrier.
fn bare_light(w: usize, h: usize, k: [f32; 3]) -> ImageF32 {
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            data.extend(light_at(x, y, (w, h), k));
        }
    }
    ImageF32::new(w as u32, h as u32, 3, data).unwrap()
}

/// A film carrier: opaque borders (left, right, top, bottom, in pixels) with a soft edge that goes
/// from dark to open over `2 * soft` pixels, a little light (`stray`) leaking through the opaque part.
#[derive(Clone, Copy)]
struct Carrier {
    sides: [f32; 4],
    soft: f32,
    stray: f32,
}

impl Carrier {
    /// How far inside the opening the pixel is (negative in the carrier).
    fn depth(&self, x: usize, y: usize, (w, h): (usize, usize)) -> f32 {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let [l, r, t, b] = self.sides;
        // A side with no border has no edge: the frame's own edge is not the carrier's.
        let edges = [(l, px), (r, w as f32 - px), (t, py), (b, h as f32 - py)];
        edges
            .iter()
            .filter(|(border, _)| *border > 0.0)
            .map(|(border, from_edge)| from_edge - border)
            .fold(f32::INFINITY, f32::min)
    }

    fn open_share(&self, x: usize, y: usize, size: (usize, usize)) -> f32 {
        ((self.depth(x, y, size) + self.soft) / (2.0 * self.soft)).clamp(0.0, 1.0)
    }

    /// The picture of light `k` seen through the carrier, its content `content` times the light.
    fn picture(&self, (w, h): (usize, usize), k: [f32; 3], content: f32) -> ImageF32 {
        let mut data = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let open = self.open_share(x, y, (w, h));
                for light in light_at(x, y, (w, h), k) {
                    data.push(content * light * open + self.stray * (1.0 - open));
                }
            }
        }
        ImageF32::new(w as u32, h as u32, 3, data).unwrap()
    }
}

fn bits(flat: &FlatField) -> Vec<u32> {
    flat.light.iter().map(|v| v.to_bits()).collect()
}

fn assert_carrier_free_matches_the_plain_method(image: &ImageF32) {
    let plain = build_flat_field_reference(image, sources()).unwrap();
    let (flat, stats) = build_flat_field_with(image, sources(), CarrierParams::default()).unwrap();
    assert_eq!(stats.covered_samples, 0, "no carrier in this picture");
    assert_eq!(stats.covered_cells, 0);
    assert_eq!((flat.grid_w, flat.grid_h), (plain.grid_w, plain.grid_h));
    assert_eq!(bits(&flat), bits(&plain), "bit for bit");
    assert_eq!(flat, plain);
}

#[test]
fn a_capture_without_a_carrier_gives_exactly_the_plain_flat_field() {
    // Strong vignetting, down to 0.3 of the centre in every colour at the corners.
    assert_carrier_free_matches_the_plain_method(&bare_light(480, 320, [0.7, 0.7, 0.7]));
    // Different for each colour, and sizes that do not divide into the grid evenly.
    assert_carrier_free_matches_the_plain_method(&bare_light(997, 641, [0.7, 0.5, 0.2]));
    assert_carrier_free_matches_the_plain_method(&bare_light(101, 77, [0.6, 0.3, 0.0]));
    // Fewer pixels than cells, and the smallest picture.
    assert_carrier_free_matches_the_plain_method(&bare_light(40, 30, [0.5, 0.4, 0.3]));
    assert_carrier_free_matches_the_plain_method(&bare_light(2, 2, [0.1, 0.1, 0.1]));
    // Even light, and a picture with a sample that is not a number (the plain method skips it).
    let even = ImageF32::new(120, 80, 3, vec![0.8; 120 * 80 * 3]).unwrap();
    assert_carrier_free_matches_the_plain_method(&even);
    let mut broken = bare_light(300, 200, [0.5, 0.5, 0.5]);
    broken.data[1] = f32::NAN;
    broken.data[(3 * 300 + 6) * 3] = f32::INFINITY;
    // A sampled pixel with no finite channel at all (every third pixel of the first cell).
    broken.data[0..3].fill(f32::NAN);
    assert_carrier_free_matches_the_plain_method(&broken);
}

/// The corrected picture of a frame seen through the same carrier as the bare light, over the band
/// of the opening beyond the soft edge: the worst deviation of the correction from the frame's own
/// content, relative to it.
fn worst_in_the_opening(
    flat: &FlatField,
    carrier: &Carrier,
    size: (usize, usize),
    k: [f32; 3],
) -> f32 {
    let content = 0.5;
    let mut frame = carrier.picture(size, k, content);
    assert!(apply_flat_field(&mut frame, flat).unwrap());
    let mut worst = 0.0f32;
    for y in 0..size.1 {
        for x in 0..size.0 {
            if carrier.depth(x, y, size) <= carrier.soft + 1.0 {
                continue;
            }
            for ch in 0..3 {
                let v = frame.data[(y * size.0 + x) * 3 + ch];
                worst = worst.max((v / content - 1.0).abs());
            }
        }
    }
    worst
}

#[test]
fn the_picture_is_even_to_the_edge_of_the_opening_where_the_plain_method_is_not() {
    let size = (960usize, 640usize);
    let k = [0.4, 0.3, 0.2];
    // An opaque border of 15 % on each side, a soft edge of 2 to 6 pixels, a little stray light.
    for soft in [1.0f32, 3.0] {
        let carrier = Carrier {
            sides: [0.15 * 960.0, 0.15 * 960.0, 0.15 * 640.0, 0.15 * 640.0],
            soft,
            stray: 0.01,
        };
        let bare = carrier.picture(size, k, 1.0);
        let image = &bare;
        let (flat, stats) =
            build_flat_field_with(image, sources(), CarrierParams::default()).unwrap();
        let plain = build_flat_field_reference(image, sources()).unwrap();
        assert!(flat.is_valid() && plain.is_valid());
        assert!(stats.covered_samples > 0 && stats.covered_cells > 0);

        let share = 0.03;
        let new = worst_in_the_opening(&flat, &carrier, size, k);
        let old = worst_in_the_opening(&plain, &carrier, size, k);
        assert!(new < share, "soft {soft}: new method deviates by {new}");
        assert!(
            old > share,
            "soft {soft}: the plain method must show the band, deviation {old}"
        );
        println!("soft edge {soft}: plain {old:.3}, new {new:.4}");
    }
}

/// The cells the carrier covers take the mean of their filled neighbours, ring by ring; checked
/// for those with an open neighbour (the first ring), where the rule gives the value exactly.
fn assert_first_ring_is_the_neighbour_mean(flat: &FlatField, stats: &BuildStats) {
    assert!(flat.is_valid());
    assert!(flat
        .light
        .iter()
        .all(|v| v.is_finite() && *v >= FlatField::MIN_LIGHT));
    let (gw, gh) = (flat.grid_w as usize, flat.grid_h as usize);
    let mut checked = 0;
    for gy in 0..gh {
        for gx in 0..gw {
            if !stats.cell_covered[gy * gw + gx] {
                continue;
            }
            let mut open = Vec::new();
            for ny in gy.saturating_sub(1)..=(gy + 1).min(gh - 1) {
                for nx in gx.saturating_sub(1)..=(gx + 1).min(gw - 1) {
                    if !stats.cell_covered[ny * gw + nx] {
                        open.push(ny * gw + nx);
                    }
                }
            }
            if open.is_empty() {
                continue;
            }
            checked += 1;
            for ch in 0..3 {
                let mean =
                    open.iter().map(|i| flat.light[i * 3 + ch]).sum::<f32>() / open.len() as f32;
                let got = flat.light[(gy * gw + gx) * 3 + ch];
                assert!(
                    (got - mean).abs() < 1e-5,
                    "cell {gx},{gy} channel {ch}: {got} against the mean {mean} of {} open cells",
                    open.len()
                );
            }
        }
    }
    assert!(checked > 0, "some cell was filled from an open neighbour");
}

#[test]
fn cells_a_carrier_covers_are_filled_from_the_open_ones() {
    let size = (480usize, 320usize);
    let k = [0.4, 0.3, 0.2];
    let carriers = [
        // One side only.
        [0.2 * 480.0, 0.0, 0.0, 0.0],
        // The top rows only.
        [0.0, 0.0, 0.1 * 320.0, 0.0],
        // One corner: two borders that meet.
        [0.25 * 480.0, 0.0, 0.25 * 320.0, 0.0],
        // The right and bottom borders: the opposite corner.
        [0.0, 0.2 * 480.0, 0.0, 0.2 * 320.0],
    ];
    for sides in carriers {
        let carrier = Carrier {
            sides,
            soft: 2.0,
            stray: 0.01,
        };
        let image = carrier.picture(size, k, 1.0);
        let (flat, stats) =
            build_flat_field_with(&image, sources(), CarrierParams::default()).unwrap();
        assert!(stats.covered_cells > 0, "{sides:?}");
        assert!(stats.excluded_samples >= stats.covered_samples);
        assert_first_ring_is_the_neighbour_mean(&flat, &stats);
        // The cells further out are filled too, and are not lower than their open neighbours' range.
        let corrected = worst_in_the_opening(&flat, &carrier, size, k);
        // The fill is the nearest open light, so it trails the gradient by the width of the margin
        // (a small frame has a steeper gradient per pixel than the 960 x 640 one above).
        assert!(corrected < 0.04, "{sides:?}: {corrected}");
    }
}

#[test]
fn a_picture_with_a_covered_centre_or_no_open_light_is_refused() {
    // Eight of the nine centre cells are black, the ninth bright: the centre is covered.
    let (w, h) = (480usize, 320usize);
    let mut data = vec![1.0f32; w * h * 3];
    let (cw, ch) = (w / 96, h / 64);
    for y in (32 - 1) * ch..(32 + 2) * ch {
        for x in (48 - 1) * cw..(48 + 2) * cw {
            let bright = x >= (48 + 1) * cw && y >= (32 + 1) * ch;
            if !bright {
                data[(y * w + x) * 3..(y * w + x) * 3 + 3].fill(0.0);
            }
        }
    }
    let image = ImageF32::new(w as u32, h as u32, 3, data).unwrap();
    let err = build_flat_field(&image, sources()).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    assert!(err.message.contains("centre"), "{}", err.message);

    // Black everywhere but a sparse bright lattice: every bright sample has a dark one beside it, so
    // nothing is open.
    let mut data = vec![0.0f32; w * h * 3];
    for y in (0..h).step_by(30) {
        for x in (0..w).step_by(30) {
            data[(y * w + x) * 3..(y * w + x) * 3 + 3].fill(1.0);
        }
    }
    let image = ImageF32::new(w as u32, h as u32, 3, data).unwrap();
    let err = build_flat_field(&image, sources()).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);

    // A black capture is still too dark to use.
    let dark = ImageF32::new(120, 80, 3, vec![0.0; 120 * 80 * 3]).unwrap();
    assert_eq!(
        build_flat_field(&dark, sources()).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn light_below_the_carrier_level_without_a_carrier_is_treated_as_covered() {
    // The right third of the frame has 20 % of the light: below the level, so it is filled from the
    // open cells (a limit decision 0021 accepts).
    let (w, h) = (480usize, 320usize);
    let mut image = ImageF32::new(w as u32, h as u32, 3, vec![1.0; w * h * 3]).unwrap();
    for y in 0..h {
        for x in 2 * w / 3..w {
            image.data[(y * w + x) * 3..(y * w + x) * 3 + 3].fill(0.2);
        }
    }
    let (flat, stats) = build_flat_field_with(&image, sources(), CarrierParams::default()).unwrap();
    assert!(stats.covered_cells > 0);
    assert!(flat.light.iter().all(|v| (v - 1.0).abs() < 1e-6));
}

#[test]
fn the_floor_holds_and_the_field_is_valid_for_every_scene() {
    let size = (480usize, 320usize);
    let carrier = Carrier {
        sides: [90.0, 90.0, 60.0, 60.0],
        soft: 2.0,
        stray: 0.0,
    };
    for k in [[0.0; 3], [0.7, 0.7, 0.7], [0.9, 0.5, 0.1]] {
        let image = carrier.picture(size, k, 1.0);
        let flat = build_flat_field(&image, sources()).unwrap();
        assert!(flat.is_valid());
        assert!(flat.light.iter().all(|v| *v >= FlatField::MIN_LIGHT));
    }
}

#[test]
fn the_result_is_the_same_on_one_thread_and_on_several() {
    let size = (960usize, 640usize);
    let carrier = Carrier {
        sides: [140.0, 130.0, 90.0, 100.0],
        soft: 3.0,
        stray: 0.01,
    };
    let image = carrier.picture(size, [0.5, 0.4, 0.3], 1.0);
    let build_on = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| build_flat_field_with(&image, sources(), CarrierParams::default()))
            .unwrap()
    };
    let (one, one_stats) = build_on(1);
    for threads in [2, 3, 8] {
        let (many, stats) = build_on(threads);
        assert_eq!(bits(&many), bits(&one), "{threads} threads");
        assert_eq!(stats.cell_covered, one_stats.cell_covered);
        assert_eq!(stats.excluded_samples, one_stats.excluded_samples);
    }
}

#[test]
fn the_margin_is_the_chebyshev_distance_on_the_map_of_samples() {
    let (width, height) = (23usize, 17usize);
    let mut covered = vec![0u8; width * height];
    for (x, y) in [(3, 4), (20, 15), (0, 16), (11, 0)] {
        covered[y * width + x] = 1;
    }
    for margin in [0usize, 1, 2, 4, 8] {
        let got = dilate(&covered, width, margin);
        for y in 0..height {
            for x in 0..width {
                let near = [(3usize, 4usize), (20, 15), (0, 16), (11, 0)]
                    .iter()
                    .any(|(cx, cy)| cx.abs_diff(x).max(cy.abs_diff(y)) <= margin);
                assert_eq!(got[y * width + x] != 0, near, "{x},{y} margin {margin}");
            }
        }
    }
}
