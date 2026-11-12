//! Flat-field correction of camera triplets (P-21, decisions/0010-flat-field.md).
//!
//! A bare-light triplet (the same three exposures with no film in the carrier) records how uneven
//! the light is across the frame, for each colour. [`build_flat_field`] reduces one to a smooth grid;
//! [`apply_flat_field`] divides a frame by it, as the frame is loaded, so the originals are never
//! changed. A roll has at most one, kept in the catalog (see [`flat_field_import`]).

use std::path::{Path, PathBuf};

use gtp_core::{AppError, ErrorCode, FlatField, ImageF32, RollId};
use gtp_ingest::merge_triplet;
use gtp_io::{Limits, RawDecoder, RawlerDecoder};
use rayon::prelude::*;
use serde::Serialize;

use crate::services::develop::read_scan;
use crate::services::library::Library;
use crate::services::roll_frame::{ensure_roll_base_from_image, roll_set_film_base};
use crate::services::triplets::triplet_group;

/// Cells across the grid; the cells down follow the frame's shape. A coarse grid is the point: the
/// unevenness of light is smooth, and dust or noise in the capture must not reach the frames.
pub const GRID_WIDTH: u32 = 96;
/// Every how many pixels of a cell is looked at when its mean is taken.
const SAMPLE_STEP: usize = 3;

/// What the UI is told of a roll's flat field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FlatFieldInfo {
    /// The size in pixels of the frames it fits.
    pub frame_width: u32,
    pub frame_height: u32,
    /// The names of the three files it was made from.
    pub sources: Vec<String>,
}

impl From<&FlatField> for FlatFieldInfo {
    fn from(flat: &FlatField) -> Self {
        Self {
            frame_width: flat.frame_w,
            frame_height: flat.frame_h,
            sources: flat.sources.clone(),
        }
    }
}

fn invalid(message: &str, details: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message.to_owned()).with_details(details.into())
}

/// Reduces a merged bare-light picture (3 channels, linear) to a flat field: the mean of each
/// channel in every cell of the grid, divided by the channel's mean over the centre (3 x 3 cells),
/// and never below [`FlatField::MIN_LIGHT`].
pub fn build_flat_field(image: &ImageF32, sources: Vec<String>) -> Result<FlatField, AppError> {
    if image.channels != 3 {
        return Err(invalid(
            "A flat field needs a picture of three colours.",
            format!("{} channels", image.channels),
        ));
    }
    let (w, h) = (image.width as usize, image.height as usize);
    let grid_w = (GRID_WIDTH as usize).min(w);
    let grid_h = ((h * grid_w).div_ceil(w)).clamp(2, h);
    if grid_w < 2 || h < 2 {
        return Err(invalid(
            "The picture is too small to make a flat field from.",
            format!("{w}x{h}"),
        ));
    }

    let cell = |ch: usize, gx: usize, gy: usize| -> f64 {
        let (x0, x1) = (
            gx * w / grid_w,
            ((gx + 1) * w / grid_w).max(gx * w / grid_w + 1),
        );
        let (y0, y1) = (
            gy * h / grid_h,
            ((gy + 1) * h / grid_h).max(gy * h / grid_h + 1),
        );
        let (mut sum, mut n) = (0.0f64, 0u64);
        for y in (y0..y1).step_by(SAMPLE_STEP) {
            for x in (x0..x1).step_by(SAMPLE_STEP) {
                let v = image.data[(y * w + x) * 3 + ch];
                if v.is_finite() {
                    sum += f64::from(v);
                    n += 1;
                }
            }
        }
        sum / n.max(1) as f64
    };

    let mut light = vec![0.0f32; grid_w * grid_h * 3];
    for ch in 0..3 {
        let means: Vec<f64> = (0..grid_h)
            .flat_map(|gy| (0..grid_w).map(move |gx| (gx, gy)))
            .map(|(gx, gy)| cell(ch, gx, gy))
            .collect();
        let (cx, cy) = (grid_w / 2, grid_h / 2);
        let centre: f64 = (cy.saturating_sub(1)..=(cy + 1).min(grid_h - 1))
            .flat_map(|gy| {
                (cx.saturating_sub(1)..=(cx + 1).min(grid_w - 1)).map(move |gx| (gx, gy))
            })
            .map(|(gx, gy)| means[gy * grid_w + gx])
            .sum::<f64>()
            / 9.0;
        if centre <= 1e-6 || !centre.is_finite() {
            return Err(invalid(
                "The bare-light capture is too dark to use.",
                format!("channel {ch}: centre {centre}"),
            ));
        }
        for (i, mean) in means.iter().enumerate() {
            light[i * 3 + ch] = ((*mean / centre) as f32).max(FlatField::MIN_LIGHT);
        }
    }
    let flat = FlatField {
        grid_w: grid_w as u32,
        grid_h: grid_h as u32,
        frame_w: image.width,
        frame_h: image.height,
        light,
        sources,
    };
    if flat.is_valid() {
        Ok(flat)
    } else {
        Err(AppError::internal("the flat field built is not valid"))
    }
}

/// Divides the picture by the flat field, each channel by its own light, interpolated between the
/// cells of the grid. Returns false and leaves the picture alone when it is not the size the flat
/// field was made for.
pub fn apply_flat_field(image: &mut ImageF32, flat: &FlatField) -> Result<bool, AppError> {
    if image.channels != 3 {
        return Err(invalid(
            "A flat field is applied to a picture of three colours.",
            format!("{} channels", image.channels),
        ));
    }
    if !flat.is_valid() {
        return Err(invalid("This flat field is not usable.", String::new()));
    }
    if (image.width, image.height) != (flat.frame_w, flat.frame_h) {
        return Ok(false);
    }
    let (w, h) = (image.width as usize, image.height as usize);
    let (gw, gh) = (flat.grid_w as usize, flat.grid_h as usize);

    // Where each column and each row falls between two cells, once.
    let spans = |len: usize, cells: usize| -> Vec<(usize, usize, f32)> {
        (0..len)
            .map(|i| {
                let f = ((i as f32 + 0.5) * cells as f32 / len as f32 - 0.5)
                    .clamp(0.0, (cells - 1) as f32);
                let lo = f.floor() as usize;
                (lo, (lo + 1).min(cells - 1), f.fract())
            })
            .collect()
    };
    let (columns, rows) = (spans(w, gw), spans(h, gh));
    let light = &flat.light;

    image
        .data
        .par_chunks_mut(w * 3)
        .zip(rows.par_iter())
        .for_each(|(row, (y0, y1, ty))| {
            for (x, (x0, x1, tx)) in columns.iter().enumerate() {
                for ch in 0..3 {
                    let at = |gy: usize, gx: usize| light[(gy * gw + gx) * 3 + ch];
                    let top = at(*y0, *x0) * (1.0 - tx) + at(*y0, *x1) * tx;
                    let bottom = at(*y1, *x0) * (1.0 - tx) + at(*y1, *x1) * tx;
                    row[x * 3 + ch] /= top * (1.0 - ty) + bottom * ty;
                }
            }
        });
    Ok(true)
}

/// The flat field of three bare-light exposures (red, green, blue, in that order) decoded by
/// `decoder`, without a roll: what [`flat_field_attach`] stores.
pub fn flat_field_from_exposures(
    exposures: [&Path; 3],
    decoder: &dyn RawDecoder,
) -> Result<FlatField, AppError> {
    let [r, g, b] = exposures;
    let merged = merge_triplet(decoder, r, g, b, &Limits::default())?.image;
    let name = |path: &Path| {
        path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
    build_flat_field(&merged, vec![name(r), name(g), name(b)])
}

/// Makes a flat field from three bare-light exposures (any order: they are told apart by colour) and
/// attaches it to the roll, replacing the one it has (see [`flat_field_attach`]). Does not touch the
/// roll's frames or thumbnails; see [`remeasure_roll_base`].
pub fn flat_field_import(
    library: &Library,
    roll: RollId,
    paths: &[PathBuf],
) -> Result<FlatFieldInfo, AppError> {
    if paths.len() != 3 {
        return Err(invalid(
            "A flat field is three files: the red-, green- and blue-lit exposure.",
            format!("{} files", paths.len()),
        ));
    }
    library.read(|c| c.get_roll(roll))?;
    let candidates = triplet_group(paths)?;
    let [candidate] = candidates.as_slice() else {
        return Err(invalid(
            "These files are not the three exposures of one capture.",
            format!("{} groups", candidates.len()),
        ));
    };
    let [r, g, b] = candidate.files.as_slice() else {
        return Err(invalid(
            "These files are not the three exposures of one capture.",
            format!("{} files in the group", candidate.files.len()),
        ));
    };
    if candidate.problem.is_some() {
        return Err(invalid(
            "These files are not the red, green and blue exposures of one capture.",
            format!("{:?}", candidate.problem),
        ));
    }
    flat_field_attach(
        library,
        roll,
        [Path::new(&r.path), Path::new(&g.path), Path::new(&b.path)],
        &RawlerDecoder,
    )
}

/// Makes a flat field from the three exposures (red, green, blue, in that order) decoded by
/// `decoder` and attaches it to the roll, replacing the one it has. The roll's frames, if it has
/// any, must be the size of the exposures.
pub fn flat_field_attach(
    library: &Library,
    roll: RollId,
    exposures: [&Path; 3],
    decoder: &dyn RawDecoder,
) -> Result<FlatFieldInfo, AppError> {
    let [r, g, b] = exposures;
    let merged = merge_triplet(decoder, r, g, b, &Limits::default())?.image;
    let sizes: Vec<(u32, u32)> = library
        .read(|c| c.list_frames(roll))?
        .iter()
        .map(|f| (f.width, f.height))
        .collect();
    if let Some((fw, fh)) = sizes
        .into_iter()
        .find(|s| *s != (merged.width, merged.height))
    {
        return Err(invalid(
            "This flat field is not the size of the roll's frames.",
            format!(
                "{}x{} against frames of {fw}x{fh}",
                merged.width, merged.height
            ),
        ));
    }
    let name = |path: &Path| {
        path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
    let flat = build_flat_field(&merged, vec![name(r), name(g), name(b)])?;
    library.write(|c| c.set_flat_field(roll, &flat))?;
    Ok(FlatFieldInfo::from(&flat))
}

/// Removes the roll's flat field; true when it had one.
pub fn flat_field_clear(library: &Library, roll: RollId) -> Result<bool, AppError> {
    library.write(|c| c.clear_flat_field(roll))
}

/// After a flat field was attached or removed: an automatic film base of the roll was measured on
/// frames as they were, so it is measured again from the roll's first frame as it is loaded now.
/// A base picked by hand is left alone. True when it was measured again.
pub fn remeasure_roll_base(library: &Library, roll: RollId) -> Result<bool, AppError> {
    let current = library.read(|c| c.get_roll(roll))?;
    if current.film_base_rgb.is_none() || current.film_base_confidence.is_none() {
        return Ok(false);
    }
    roll_set_film_base(library, roll, None)?;
    let first = library.read(|c| Ok(c.list_frames(roll)?.into_iter().next()))?;
    let Some(frame) = first else {
        return Ok(true);
    };
    let (sources, flat) =
        library.read(|c| Ok((c.sources_of(frame.id)?, c.get_flat_field(roll)?)))?;
    let image = read_scan(library, &sources, flat.as_ref())?;
    ensure_roll_base_from_image(library, roll, &image)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::testkit::TempDir;

    /// A picture whose light falls off towards the left and right edges, more for red than for
    /// green, and not at all for blue.
    fn vignetted(w: u32, h: u32) -> ImageF32 {
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for _y in 0..h {
            for x in 0..w {
                let u = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0; // -1 at the left edge, 1 at the right
                let edge = u * u;
                data.extend([1.0 - 0.6 * edge, 1.0 - 0.3 * edge, 1.0]);
            }
        }
        ImageF32::new(w, h, 3, data).unwrap()
    }

    fn sources() -> Vec<String> {
        vec!["r.NEF".into(), "g.NEF".into(), "b.NEF".into()]
    }

    #[test]
    fn a_flat_field_is_the_light_over_the_centre_of_each_channel() {
        let flat = build_flat_field(&vignetted(480, 320), sources()).unwrap();
        assert_eq!((flat.grid_w, flat.grid_h), (96, 64));
        assert_eq!((flat.frame_w, flat.frame_h), (480, 320));
        assert!(flat.is_valid());
        let at = |gx: usize, gy: usize, ch: usize| flat.light[(gy * 96 + gx) * 3 + ch];
        // The centre is 1 for every channel; the left edge has less red than green, blue unchanged.
        for ch in 0..3 {
            assert!((at(48, 32, ch) - 1.0).abs() < 0.01, "channel {ch}");
        }
        let (red, green, blue) = (at(0, 32, 0), at(0, 32, 1), at(0, 32, 2));
        assert!(red < green && green < blue, "{red} {green} {blue}");
        assert!(
            (red - 0.4).abs() < 0.02 && (green - 0.7).abs() < 0.02 && (blue - 1.0).abs() < 0.01
        );
        assert_eq!(flat.sources, sources());
    }

    #[test]
    fn dividing_the_picture_by_its_own_flat_field_makes_it_even() {
        let image = vignetted(480, 320);
        let flat = build_flat_field(&image, sources()).unwrap();
        let mut corrected = image.clone();
        assert!(apply_flat_field(&mut corrected, &flat).unwrap());
        // Every pixel is about 1, edges included; what is left is the grid's own resolution, where the
        let worst = corrected
            .data
            .iter()
            .map(|v| (v - 1.0).abs())
            .fold(0.0, f32::max);
        // light bends most at the very edge (a cell mean is not the value at its pixels).
        assert!(worst < 0.04, "worst deviation {worst}");
    }

    #[test]
    fn a_uniform_flat_field_changes_nothing() {
        let even = ImageF32::new(120, 80, 3, vec![0.8; 120 * 80 * 3]).unwrap();
        let flat = build_flat_field(&even, sources()).unwrap();
        assert!(flat.light.iter().all(|v| (v - 1.0).abs() < 1e-6));
        let picture = vignetted(120, 80);
        let mut same = picture.clone();
        assert!(apply_flat_field(&mut same, &flat).unwrap());
        for (a, b) in same.data.iter().zip(&picture.data) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn a_picture_of_another_size_is_left_alone_and_bad_input_is_refused() {
        let flat = build_flat_field(&vignetted(120, 80), sources()).unwrap();
        let mut other = vignetted(100, 80);
        let before = other.clone();
        assert!(!apply_flat_field(&mut other, &flat).unwrap());
        assert_eq!(other, before);

        let gray = ImageF32::new(120, 80, 1, vec![0.5; 120 * 80]).unwrap();
        assert_eq!(
            build_flat_field(&gray, sources()).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let mut gray_mut = gray;
        assert_eq!(
            apply_flat_field(&mut gray_mut, &flat).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let dark = ImageF32::new(120, 80, 3, vec![0.0; 120 * 80 * 3]).unwrap();
        assert_eq!(
            build_flat_field(&dark, sources()).unwrap_err().code,
            ErrorCode::InvalidInput,
            "a black capture cannot be divided by"
        );
        let mut broken = flat.clone();
        broken.light.pop();
        assert!(apply_flat_field(&mut vignetted(120, 80), &broken).is_err());
    }

    #[test]
    fn the_light_never_goes_below_its_floor() {
        // The outer columns are black, as the carrier is.
        let mut data = Vec::new();
        for _y in 0..80 {
            for x in 0..120 {
                let v = if !(6..114).contains(&x) { 0.0 } else { 1.0 };
                data.extend([v, v, v]);
            }
        }
        let image = ImageF32::new(120, 80, 3, data).unwrap();
        let flat = build_flat_field(&image, sources()).unwrap();
        assert!(flat.light.iter().all(|v| *v >= FlatField::MIN_LIGHT));
        let mut corrected = image.clone();
        apply_flat_field(&mut corrected, &flat).unwrap();
        assert!(corrected.data.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn files_that_are_not_one_bare_light_triplet_are_refused() {
        let temp = TempDir::new("flat-import");
        let dir = temp.path();
        let lib = Library::create(&dir.join("lib"), "Flat").unwrap();
        let roll = crate::services::roll_frame::roll_create(
            &lib,
            crate::services::roll_frame::RollInput {
                name: "R".into(),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
        let files: Vec<PathBuf> = ["a.NEF", "b.NEF", "c.NEF"]
            .iter()
            .map(|n| {
                let p = dir.join(n);
                std::fs::write(&p, b"not a camera file").unwrap();
                p
            })
            .collect();
        // Two files, no files, a missing roll, and three files that are not camera pictures.
        assert_eq!(
            flat_field_import(&lib, roll, &files[..2]).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        assert_eq!(
            flat_field_import(&lib, roll, &[]).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        assert_eq!(
            flat_field_import(&lib, RollId::new(), &files)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            flat_field_import(&lib, roll, &files).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        assert!(!lib.read(|c| c.get_roll(roll)).unwrap().has_flat_field);
        assert!(!flat_field_clear(&lib, roll).unwrap());
    }
}
