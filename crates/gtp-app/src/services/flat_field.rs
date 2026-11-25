//! Flat-field correction of camera triplets (P-21, decisions/0010-flat-field.md).
//!
//! A bare-light triplet (the same three exposures with no film in the carrier) records how uneven
//! the light is across the frame, for each colour. [`build_flat_field`] reduces one to a smooth grid;
//! [`apply_flat_field`] divides a frame by it, as the frame is loaded, so the originals are never
//! changed. A roll has at most one, kept in the catalog (see [`flat_field_import`]).
//!
//! The film carrier is not light: samples of the capture that it covers, and a margin round them,
//! are left out of the cell means, and cells that are mostly covered are filled from their open
//! neighbours (P-35, decisions/0021-flat-field-ignores-the-carrier.md). [`flat_field_rebuild`]
//! makes a flat field again from the files it kept, with the current method.

use std::path::{Path, PathBuf};

use std::sync::Arc;

use gtp_core::{AppError, ErrorCode, ExposureInfo, FlatField, FrameId, ImageF32, RollId};
use gtp_ingest::{merge_triplet, read_exposure_info};
use gtp_io::{mark_read_only, Limits, RawDecoder, RawlerDecoder};
use rayon::prelude::*;

use crate::context::ThumbnailRequester;
use crate::jobs::LibraryEvents;
use crate::library_fs::flat_field_rel;
use crate::services::capture_levels::exposure_from_exif;
use crate::services::develop::read_scan;
use crate::services::library::Library;
use crate::services::roll_frame::{
    ensure_roll_base_from_image, remove_in_library, roll_set_film_base,
};
use crate::services::triplets::triplet_group;

/// Cells across the grid; the cells down follow the frame's shape. A coarse grid is the point: the
/// unevenness of light is smooth, and dust or noise in the capture must not reach the frames.
pub const GRID_WIDTH: u32 = 96;
/// Every how many pixels of a cell is looked at when its mean is taken.
const SAMPLE_STEP: usize = 3;
/// A sample whose brightest channel is below this share of the channel's centre level is film
/// carrier, not light (decisions/0021-flat-field-ignores-the-carrier.md).
pub const CARRIER_LEVEL: f64 = 0.25;
/// Open samples this close to a carrier sample (Chebyshev distance on the map of samples, so about
/// three pixels each) are left out too: the carrier's soft edge and the flare beside it.
pub const CARRIER_MARGIN_SAMPLES: usize = 4;
/// A cell with a smaller share of open samples than this is covered and is filled from its
/// neighbours.
pub const MIN_OPEN_SHARE: f64 = 0.25;

pub use gtp_core::{FlatFieldInfo, FlatFieldSummary};

fn invalid(message: &str, details: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message.to_owned()).with_details(details.into())
}

/// Reduces a merged bare-light picture (3 channels, linear) to a flat field: the mean of each
/// channel in every cell of the grid, divided by the channel's mean over the centre (3 x 3 cells),
/// and never below [`FlatField::MIN_LIGHT`].
///
/// The film carrier is left out (P-35, decisions/0021-flat-field-ignores-the-carrier.md). A sample
/// (every [`SAMPLE_STEP`]-th pixel in x and y) is covered when its brightest channel is below
/// [`CARRIER_LEVEL`] of the channel's centre level; open samples within [`CARRIER_MARGIN_SAMPLES`]
/// of a covered one are left out too (the soft edge and flare of the carrier). A cell is the mean of
/// its remaining samples; a cell with less than [`MIN_OPEN_SHARE`] of its samples left is covered,
/// and is filled from the open cells ring by ring (the mean of its filled 8-neighbours). A picture
/// with no covered sample gives exactly the cell means of the plain method. A picture whose centre
/// is covered, or none of whose cells is open, is refused.
pub fn build_flat_field(image: &ImageF32, sources: Vec<String>) -> Result<FlatField, AppError> {
    build_flat_field_with(image, sources, CarrierParams::default()).map(|(flat, _)| flat)
}

/// What decides which samples are the carrier.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CarrierParams {
    /// A sample below this share of the centre level (brightest channel) is covered.
    pub level: f64,
    /// Open samples this close (in samples, Chebyshev) to a covered one are left out too.
    pub margin: usize,
}

impl Default for CarrierParams {
    fn default() -> Self {
        Self {
            level: CARRIER_LEVEL,
            margin: CARRIER_MARGIN_SAMPLES,
        }
    }
}

/// What a build found, for the measurements of decision 0021.
#[derive(Debug, Clone, Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct BuildStats {
    /// Samples taken, and samples that were covered (before the margin).
    pub samples: usize,
    pub covered_samples: usize,
    /// Samples left out, margin included.
    pub excluded_samples: usize,
    /// Cells that were filled instead of measured.
    pub covered_cells: usize,
    /// For each cell, whether it was covered (row by row).
    pub cell_covered: Vec<bool>,
    /// The samples left out (1), row by row of the map, and the pixel column and row of each
    /// sample of the map.
    pub excluded: Vec<u8>,
    pub sample_x: Vec<usize>,
    pub sample_y: Vec<usize>,
    /// The samples of the map each cell column and each cell row owns.
    pub cell_columns: Vec<std::ops::Range<usize>>,
    pub cell_rows: Vec<std::ops::Range<usize>>,
}

/// The samples along one axis: the pixels `a, a + SAMPLE_STEP, ...` of every cell column (or row),
/// in order, and the range of them each cell owns.
struct SampleAxis {
    at: Vec<usize>,
    cells: Vec<std::ops::Range<usize>>,
}

impl SampleAxis {
    fn new(len: usize, cells: usize) -> Self {
        let (mut at, mut ranges) = (Vec::new(), Vec::with_capacity(cells));
        for g in 0..cells {
            let a = g * len / cells;
            let b = ((g + 1) * len / cells).max(a + 1);
            let first = at.len();
            at.extend((a..b).step_by(SAMPLE_STEP));
            ranges.push(first..at.len());
        }
        Self { at, cells: ranges }
    }
}

/// Sum and count of finite values of one cell, per channel, and the number of samples left out.
/// `excluded` marks the samples to skip (`None`: none). The order is the plain method's: row by
/// row, so that nothing left out gives its sums exactly.
fn cell_sums(
    image: &ImageF32,
    (xs, ys): (&SampleAxis, &SampleAxis),
    (gx, gy): (usize, usize),
    excluded: Option<&[u8]>,
) -> ([f64; 3], [u64; 3], usize) {
    let (w, n_x) = (image.width as usize, xs.at.len());
    let (mut sum, mut n, mut left_out) = ([0.0f64; 3], [0u64; 3], 0usize);
    for sy in ys.cells[gy].clone() {
        for sx in xs.cells[gx].clone() {
            if excluded.is_some_and(|e| e[sy * n_x + sx] != 0) {
                left_out += 1;
                continue;
            }
            let base = (ys.at[sy] * w + xs.at[sx]) * 3;
            for ch in 0..3 {
                let v = image.data[base + ch];
                if v.is_finite() {
                    sum[ch] += f64::from(v);
                    n[ch] += 1;
                }
            }
        }
    }
    (sum, n, left_out)
}

/// The samples of `covered` and those within `margin` samples of one (Chebyshev distance), as 1:
/// a pass along the rows, then one along the columns.
fn dilate(covered: &[u8], width: usize, margin: usize) -> Vec<u8> {
    if margin == 0 || covered.is_empty() {
        return covered.to_vec();
    }
    let height = covered.len() / width;
    let mut along = vec![0u8; covered.len()];
    along
        .par_chunks_mut(width)
        .zip(covered.par_chunks(width))
        .for_each(|(out, row)| {
            // Prefix counts make "any covered in x - margin ..= x + margin" a subtraction.
            let mut prefix = Vec::with_capacity(width + 1);
            prefix.push(0u32);
            for v in row {
                prefix.push(prefix[prefix.len() - 1] + u32::from(*v != 0));
            }
            for (x, o) in out.iter_mut().enumerate() {
                let (a, b) = (x.saturating_sub(margin), (x + margin + 1).min(width));
                *o = u8::from(prefix[b] > prefix[a]);
            }
        });
    let mut out = vec![0u8; covered.len()];
    out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        for yy in y.saturating_sub(margin)..(y + margin + 1).min(height) {
            for (o, v) in row.iter_mut().zip(&along[yy * width..(yy + 1) * width]) {
                *o |= *v;
            }
        }
    });
    out
}

/// [`build_flat_field`] with the carrier rule given, and what it found.
pub(crate) fn build_flat_field_with(
    image: &ImageF32,
    sources: Vec<String>,
    params: CarrierParams,
) -> Result<(FlatField, BuildStats), AppError> {
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
    let (xs, ys) = (SampleAxis::new(w, grid_w), SampleAxis::new(h, grid_h));
    let axes = (&xs, &ys);

    // The centre level of each channel: the mean over the centre cells, every sample counted.
    let (cx, cy) = (grid_w / 2, grid_h / 2);
    let centre_cells: Vec<(usize, usize)> = (cy.saturating_sub(1)..=(cy + 1).min(grid_h - 1))
        .flat_map(|gy| (cx.saturating_sub(1)..=(cx + 1).min(grid_w - 1)).map(move |gx| (gx, gy)))
        .collect();
    let mut centre = [0.0f64; 3];
    for (ch, level) in centre.iter_mut().enumerate() {
        let total: f64 = centre_cells
            .iter()
            .map(|cell| {
                let (sum, n, _) = cell_sums(image, axes, *cell, None);
                sum[ch] / n[ch].max(1) as f64
            })
            .sum();
        *level = total / 9.0;
        if *level <= 1e-6 || !level.is_finite() {
            return Err(invalid(
                "The bare-light capture is too dark to use.",
                format!("channel {ch}: centre {level}"),
            ));
        }
    }

    // Covered samples, then the margin round them.
    let (n_x, n_y) = (xs.at.len(), ys.at.len());
    let mut covered = vec![0u8; n_x * n_y];
    covered
        .par_chunks_mut(n_x)
        .zip(ys.at.par_iter())
        .for_each(|(row, y)| {
            for (o, x) in row.iter_mut().zip(&xs.at) {
                let base = (y * w + x) * 3;
                let mut brightest = f64::NEG_INFINITY;
                for (ch, level) in centre.iter().enumerate() {
                    let v = image.data[base + ch];
                    if v.is_finite() {
                        brightest = brightest.max(f64::from(v) / level);
                    }
                }
                // No finite channel says nothing about the carrier: the cell sums skip it.
                *o = u8::from(brightest.is_finite() && brightest < params.level);
            }
        });
    let covered_samples = covered.iter().filter(|v| **v != 0).count();
    let excluded = dilate(&covered, n_x, params.margin);
    let excluded_samples = excluded.iter().filter(|v| **v != 0).count();

    // Each cell: the mean of its open samples; covered when too few are open.
    let cells: Vec<([f64; 3], bool)> = (0..grid_w * grid_h)
        .into_par_iter()
        .map(|i| {
            let (gx, gy) = (i % grid_w, i / grid_w);
            let (sum, n, left_out) = cell_sums(image, axes, (gx, gy), Some(&excluded));
            let total = xs.cells[gx].len() * ys.cells[gy].len();
            let open = (total - left_out) as f64 / total.max(1) as f64;
            let mut mean = [0.0f64; 3];
            for ch in 0..3 {
                mean[ch] = sum[ch] / n[ch].max(1) as f64;
            }
            (mean, open >= MIN_OPEN_SHARE)
        })
        .collect();
    let mut means: Vec<[f64; 3]> = cells.iter().map(|(m, _)| *m).collect();
    let mut filled: Vec<bool> = cells.iter().map(|(_, open)| *open).collect();
    let cell_covered: Vec<bool> = filled.iter().map(|f| !f).collect();
    let covered_cells = cell_covered.iter().filter(|c| **c).count();
    if covered_cells == filled.len() {
        return Err(invalid(
            "The bare-light capture has no open light to measure: it is all film carrier.",
            format!("{covered_cells} cells, all covered"),
        ));
    }
    if let Some((gx, gy)) = centre_cells
        .iter()
        .find(|(gx, gy)| !filled[gy * grid_w + gx])
    {
        return Err(invalid(
            "The centre of the bare-light capture is covered, so the light cannot be measured.",
            format!("centre cell {gx},{gy} is covered"),
        ));
    }

    // Ring by ring: a covered cell next to a filled one takes the mean of its filled neighbours,
    // judged on what was filled before the pass, so the order of the cells does not matter.
    while filled.iter().any(|f| !f) {
        let mut pass: Vec<(usize, [f64; 3])> = Vec::new();
        for gy in 0..grid_h {
            for gx in 0..grid_w {
                if filled[gy * grid_w + gx] {
                    continue;
                }
                let (mut sum, mut n) = ([0.0f64; 3], 0usize);
                for ny in gy.saturating_sub(1)..=(gy + 1).min(grid_h - 1) {
                    for nx in gx.saturating_sub(1)..=(gx + 1).min(grid_w - 1) {
                        let j = ny * grid_w + nx;
                        if filled[j] {
                            for (s, m) in sum.iter_mut().zip(means[j]) {
                                *s += m;
                            }
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    pass.push((gy * grid_w + gx, sum.map(|s| s / n as f64)));
                }
            }
        }
        if pass.is_empty() {
            return Err(AppError::internal("a covered cell could not be filled"));
        }
        for (i, mean) in pass {
            means[i] = mean;
            filled[i] = true;
        }
    }

    let mut light = vec![0.0f32; grid_w * grid_h * 3];
    for (i, mean) in means.iter().enumerate() {
        for ch in 0..3 {
            light[i * 3 + ch] = ((mean[ch] / centre[ch]) as f32).max(FlatField::MIN_LIGHT);
        }
    }
    let flat = FlatField {
        grid_w: grid_w as u32,
        grid_h: grid_h as u32,
        frame_w: image.width,
        frame_h: image.height,
        light,
        sources,
        exposure: ExposureInfo::default(),
        files: Vec::new(),
        created_at: None,
    };
    if !flat.is_valid() {
        return Err(AppError::internal("the flat field built is not valid"));
    }
    let stats = BuildStats {
        samples: n_x * n_y,
        covered_samples,
        excluded_samples,
        covered_cells,
        cell_covered,
        excluded,
        sample_x: xs.at,
        sample_y: ys.at,
        cell_columns: xs.cells,
        cell_rows: ys.cells,
    };
    Ok((flat, stats))
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

/// The exposure of a bare-light triplet from the EXIF of its three files: each field is the first
/// one the files (red, green, blue) say; empty when none does.
pub(crate) fn exposure_of_files(files: [&Path; 3]) -> ExposureInfo {
    let all = files.map(|path| exposure_from_exif(read_exposure_info(path)));
    ExposureInfo {
        exposure_time: all.iter().find_map(|e| e.exposure_time),
        iso: all.iter().find_map(|e| e.iso),
        f_number: all.iter().find_map(|e| e.f_number),
    }
}

/// Makes a flat field from the three exposures (red, green, blue, in that order) decoded by
/// `decoder` and attaches it to the roll, replacing the one it has. The roll's frames, if it has
/// any, must be the size of the exposures. The flat field remembers how the files were exposed;
/// the files themselves are only read, not kept.
pub fn flat_field_attach(
    library: &Library,
    roll: RollId,
    exposures: [&Path; 3],
    decoder: &dyn RawDecoder,
) -> Result<FlatFieldInfo, AppError> {
    let [r, g, b] = exposures;
    let merged = merge_triplet(decoder, r, g, b, &Limits::default())?.image;
    let name = |path: &Path| {
        path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
    attach_merged(
        library,
        roll,
        &merged,
        vec![name(r), name(g), name(b)],
        exposure_of_files(exposures),
        Vec::new(),
    )
}

/// Refuses a flat field of `width` x `height` pixels when a frame of the roll has another size.
fn check_frames_fit(
    library: &Library,
    roll: RollId,
    width: u32,
    height: u32,
) -> Result<(), AppError> {
    let sizes: Vec<(u32, u32)> = library
        .read(|c| c.list_frames(roll))?
        .iter()
        .map(|f| (f.width, f.height))
        .collect();
    if let Some((fw, fh)) = sizes.into_iter().find(|s| *s != (width, height)) {
        return Err(invalid(
            "This flat field is not the size of the roll's frames.",
            format!("{width}x{height} against frames of {fw}x{fh}"),
        ));
    }
    Ok(())
}

/// Builds the flat field of a merged bare-light picture and attaches it to the roll, replacing the
/// one it has (and removing the files that one kept, except those in `files`, the library-relative
/// paths this one keeps). The roll's frames, if it has any, must be the size of the picture.
pub(crate) fn attach_merged(
    library: &Library,
    roll: RollId,
    merged: &ImageF32,
    sources: Vec<String>,
    exposure: ExposureInfo,
    files: Vec<String>,
) -> Result<FlatFieldInfo, AppError> {
    check_frames_fit(library, roll, merged.width, merged.height)?;
    let mut flat = build_flat_field(merged, sources)?;
    flat.exposure = exposure;
    flat.files = files;
    store_flat_field(library, roll, &flat)
}

/// Stores `flat` as the roll's flat field, replacing the one it has and removing the files that one
/// kept, except those `flat` keeps too.
fn store_flat_field(
    library: &Library,
    roll: RollId,
    flat: &FlatField,
) -> Result<FlatFieldInfo, AppError> {
    let old = library.read(|c| c.get_flat_field(roll))?;
    library.write(|c| c.set_flat_field(roll, flat))?;
    if let Some(old) = old {
        let replaced: Vec<String> = old
            .files
            .into_iter()
            .filter(|f| !flat.files.contains(f))
            .collect();
        remove_kept_files(library, roll, &replaced);
    }
    Ok(FlatFieldInfo::from(flat))
}

/// Every roll's flat field, newest first (ties by roll name): what "use another roll's flat field"
/// offers (H-26).
pub fn flat_field_list(library: &Library) -> Result<Vec<FlatFieldSummary>, AppError> {
    library.read(|c| c.list_flat_fields())
}

/// The name of a kept file for its copy: the same pattern as `flat_field_capture` makes
/// (`r|g|b-<token>-<name>`), with the original's name part and a new token.
fn copied_name(original: &str, index: usize, token: &str) -> String {
    let file = Path::new(original)
        .file_name()
        .map_or_else(|| original.to_owned(), |n| n.to_string_lossy().into_owned());
    let mut parts = file.splitn(3, '-');
    let rest = match (parts.next(), parts.next(), parts.next()) {
        (Some(prefix), Some(old), Some(rest))
            if matches!(prefix, "r" | "g" | "b") && old.len() == token.len() =>
        {
            rest.to_owned()
        }
        _ => file,
    };
    let prefix = ["r", "g", "b"].get(index).copied().unwrap_or("x");
    format!("{prefix}-{token}-{rest}")
}

/// Copies the kept files of the source into the target roll's flat-field folder under new names and
/// marks them read-only; returns the new library-relative paths. Removes what it copied on failure.
fn copy_kept_files(
    library: &Library,
    files: &[String],
    to: RollId,
) -> Result<Vec<String>, AppError> {
    let token: String = FrameId::new().to_string().chars().rev().take(8).collect();
    let dir = flat_field_rel(to);
    let rels: Vec<String> = files
        .iter()
        .enumerate()
        .map(|(i, file)| {
            dir.join(copied_name(file, i, &token))
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    let copy_all = || -> Result<(), AppError> {
        for (from, rel) in files.iter().zip(&rels) {
            let from = library.resolve(Path::new(from))?;
            let to = library.resolve(Path::new(rel))?;
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|err| AppError::from_io(&err))?;
            }
            std::fs::copy(&from, &to).map_err(|err| AppError::from_io(&err))?;
            mark_read_only(&to)?;
        }
        Ok(())
    };
    match copy_all() {
        Ok(()) => Ok(rels),
        Err(err) => {
            remove_kept_files(library, to, &rels);
            Err(err)
        }
    }
}

/// Gives the roll `to` a copy of the flat field of the roll `from`, replacing the one it has. The
/// stored picture is copied as it is (nothing is decoded again), with the date it was made. The
/// files the source kept are copied into the target's own folder, so deleting the source roll later
/// leaves the copy whole. The target's frames, if it has any, must be the size of the flat field.
/// Nothing changes when this fails. The caller makes the roll's frames and thumbnails follow (see
/// [`after_flat_field_change`]).
pub fn flat_field_copy(
    library: &Library,
    from: RollId,
    to: RollId,
) -> Result<FlatFieldInfo, AppError> {
    if from == to {
        return Err(invalid(
            "Pick another roll: a roll cannot take its own flat field.",
            "the same roll as source and target",
        ));
    }
    library.read(|c| c.get_roll(to))?;
    library.read(|c| c.get_roll(from))?;
    let Some(mut flat) = library.read(|c| c.get_flat_field(from))? else {
        return Err(invalid(
            "That roll has no flat field to copy.",
            format!("roll {from}"),
        ));
    };
    check_frames_fit(library, to, flat.frame_w, flat.frame_h)?;
    flat.files = copy_kept_files(library, &flat.files, to)?;
    match store_flat_field(library, to, &flat) {
        Ok(info) => Ok(info),
        Err(err) => {
            remove_kept_files(library, to, &flat.files);
            Err(err)
        }
    }
}

/// Builds the roll's flat field again from the files it kept, with the current method (the film
/// carrier left out, decisions/0021-flat-field-ignores-the-carrier.md): a flat field made before
/// that was built without it. The sources, exposure, kept files and date stay as they were. Refused,
/// with nothing changed, when the roll has no flat field, when it was made from files that were not
/// kept, or when a kept file is gone. The caller makes the roll's frames and thumbnails follow (see
/// [`after_flat_field_change`]).
pub fn flat_field_rebuild(library: &Library, roll: RollId) -> Result<FlatFieldInfo, AppError> {
    flat_field_rebuild_with(library, roll, &RawlerDecoder)
}

/// [`flat_field_rebuild`] with the decoder of the camera files given.
pub(crate) fn flat_field_rebuild_with(
    library: &Library,
    roll: RollId,
    decoder: &dyn RawDecoder,
) -> Result<FlatFieldInfo, AppError> {
    library.read(|c| c.get_roll(roll))?;
    let Some(old) = library.read(|c| c.get_flat_field(roll))? else {
        return Err(invalid(
            "This roll has no flat field to build again.",
            format!("roll {roll}"),
        ));
    };
    if old.files.is_empty() {
        return Err(invalid(
            "This flat field was made from files that were not kept, so it cannot be built again. Set a flat field from its three files instead.",
            "no kept files",
        ));
    }
    let [r, g, b] = old.files.as_slice() else {
        return Err(invalid(
            "This flat field does not have its three kept files, so it cannot be built again.",
            format!("{} kept files", old.files.len()),
        ));
    };
    // All three are looked for before anything is read or written.
    let mut paths = Vec::with_capacity(3);
    let mut missing = Vec::new();
    for rel in [r, g, b] {
        let path = library.resolve(Path::new(rel))?;
        if !path.is_file() {
            missing.push(rel.clone());
        }
        paths.push(path);
    }
    if !missing.is_empty() {
        return Err(invalid(
            "The files this flat field was made from are gone, so it cannot be built again.",
            missing.join(", "),
        ));
    }
    let merged = merge_triplet(decoder, &paths[0], &paths[1], &paths[2], &Limits::default())?.image;
    check_frames_fit(library, roll, merged.width, merged.height)?;
    let mut flat = build_flat_field(&merged, old.sources.clone())?;
    flat.exposure = old.exposure;
    // The list is the one stored, so storing removes none of the kept files.
    flat.files = old.files.clone();
    flat.created_at = old.created_at.clone();
    store_flat_field(library, roll, &flat)
}

/// Removes the files a flat field kept, best effort: what cannot be removed is logged and left,
/// as a roll delete does. The folder goes too when it is then empty.
pub(crate) fn remove_kept_files(library: &Library, roll: RollId, files: &[String]) {
    if files.is_empty() {
        return;
    }
    let mut left = Vec::new();
    for file in files {
        remove_in_library(library, Path::new(file), &mut left);
    }
    if let Ok(dir) = library.resolve(&flat_field_rel(roll)) {
        // Only an empty folder goes; anything else in it stays.
        let _ = std::fs::remove_dir(dir);
    }
    if !left.is_empty() {
        tracing::warn!(roll = %roll, files = ?left, "flat-field files left after removal");
    }
}

/// Removes the roll's flat field and the files it kept; true when it had one.
pub fn flat_field_clear(library: &Library, roll: RollId) -> Result<bool, AppError> {
    let old = library.read(|c| c.get_flat_field(roll))?;
    let removed = library.write(|c| c.clear_flat_field(roll))?;
    if let Some(old) = old {
        remove_kept_files(library, roll, &old.files);
    }
    Ok(removed)
}

/// After a flat field was attached or removed: the proxies Develop kept were built without it, an
/// automatic film base was measured without it, and every frame of the roll looks different, so all
/// their thumbnails are made again in the background.
pub fn after_flat_field_change(
    library: &Library,
    thumbnails: &ThumbnailRequester,
    events: Arc<dyn LibraryEvents>,
    roll: RollId,
) -> Result<(), AppError> {
    thumbnails.clear_proxies();
    remeasure_roll_base(library, roll)?;
    let frames = library
        .read(|c| c.list_frames(roll))?
        .into_iter()
        .map(|f| f.id)
        .collect();
    thumbnails.request(events, frames);
    Ok(())
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
mod carrier_tests;
#[cfg(test)]
mod measure;
#[cfg(test)]
mod reference;

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

    /// A library for the copy tests, with helpers.
    struct Pair {
        lib: Library,
        _temp: TempDir,
    }

    impl Pair {
        fn new(label: &str) -> Self {
            let temp = TempDir::new(label);
            let lib = Library::create(&temp.path().join("lib"), "Copy").unwrap();
            Self { lib, _temp: temp }
        }

        fn roll(&self, name: &str) -> RollId {
            crate::services::roll_frame::roll_create(
                &self.lib,
                crate::services::roll_frame::RollInput {
                    name: name.into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .id
        }

        /// Gives `roll` a flat field (for frames of 120 x 80) made on `date`, keeping `kept` files
        /// (written for real); returns it as the catalog has it.
        fn flat(&self, roll: RollId, date: Option<&str>, kept: &[&str]) -> FlatField {
            let mut flat = build_flat_field(&vignetted(120, 80), sources()).unwrap();
            flat.exposure.iso = Some(100);
            flat.created_at = date.map(str::to_owned);
            flat.files = kept
                .iter()
                .map(|name| {
                    let rel = flat_field_rel(roll).join(name);
                    let path = self.lib.resolve(&rel).unwrap();
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, name.as_bytes()).unwrap();
                    rel.to_string_lossy().replace('\\', "/")
                })
                .collect();
            self.lib.write(|c| c.set_flat_field(roll, &flat)).unwrap();
            self.get(roll).unwrap()
        }

        fn get(&self, roll: RollId) -> Option<FlatField> {
            self.lib.read(|c| c.get_flat_field(roll)).unwrap()
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.lib.resolve(Path::new(rel)).unwrap()
        }
    }

    #[test]
    fn a_copy_has_the_picture_the_exposure_the_date_and_files_of_its_own() {
        let p = Pair::new("flat-copy");
        let (from, to) = (p.roll("From"), p.roll("To"));
        let src = p.flat(
            from,
            Some("2020-05-01T08:00:00Z"),
            &[
                "r-AAAAAAAA-one.NEF",
                "g-AAAAAAAA-two.NEF",
                "b-AAAAAAAA-three.NEF",
            ],
        );

        let info = flat_field_copy(&p.lib, from, to).unwrap();
        let copy = p.get(to).unwrap();
        assert_eq!(info, FlatFieldInfo::from(&src));
        assert_eq!(
            (&copy.light, &copy.sources, copy.exposure, &copy.created_at),
            (&src.light, &src.sources, src.exposure, &src.created_at),
            "the same picture, shot on the same date"
        );
        assert_eq!(copy.created_at.as_deref(), Some("2020-05-01T08:00:00Z"));
        assert_eq!(copy.files.len(), 3);
        for (rel, (prefix, name)) in
            copy.files
                .iter()
                .zip([("r", "one.NEF"), ("g", "two.NEF"), ("b", "three.NEF")])
        {
            assert!(!src.files.contains(rel), "a new name");
            assert!(rel.starts_with(&format!("originals/{to}/flat-field/{prefix}-")));
            assert!(rel.ends_with(&format!("-{name}")), "{rel}");
            assert!(!rel.contains("AAAAAAAA"), "a new token: {rel}");
            let path = p.path(rel);
            assert!(std::fs::metadata(&path).unwrap().permissions().readonly());
            assert!(std::fs::read(&path).unwrap().ends_with(name.as_bytes()));
        }
        for rel in &src.files {
            assert!(p.path(rel).exists(), "the source keeps its own");
        }
    }

    #[test]
    fn a_copy_of_a_flat_field_without_kept_files_keeps_none() {
        let p = Pair::new("flat-copy-none");
        let (from, to) = (p.roll("From"), p.roll("To"));
        p.flat(from, None, &[]);
        flat_field_copy(&p.lib, from, to).unwrap();
        assert!(p.get(to).unwrap().files.is_empty());
        assert!(!p.path(&flat_field_rel(to).to_string_lossy()).exists());
    }

    #[test]
    fn a_copy_that_cannot_be_made_is_refused_and_changes_nothing() {
        let p = Pair::new("flat-copy-refused");
        let (from, to, bare) = (p.roll("From"), p.roll("To"), p.roll("Bare"));
        let old = p.flat(to, Some("2019-01-01T00:00:00Z"), &["r-OLDOLDOL-old.NEF"]);
        let code = |r: Result<FlatFieldInfo, AppError>| r.unwrap_err().code;

        assert_eq!(
            code(flat_field_copy(&p.lib, to, to)),
            ErrorCode::InvalidInput,
            "its own"
        );
        assert_eq!(
            code(flat_field_copy(&p.lib, bare, to)),
            ErrorCode::InvalidInput,
            "no flat field to copy"
        );
        assert_eq!(
            code(flat_field_copy(&p.lib, RollId::new(), to)),
            ErrorCode::NotFound
        );
        assert_eq!(
            code(flat_field_copy(&p.lib, to, RollId::new())),
            ErrorCode::NotFound
        );

        // Frames of another size: refused before any file is copied.
        p.flat(from, None, &["r-AAAAAAAA-one.NEF"]);
        crate::services::testkit::add_frame(&p.lib, to, 1, 100, 60);
        assert_eq!(
            code(flat_field_copy(&p.lib, from, to)),
            ErrorCode::InvalidInput
        );
        let dir = p.path(&flat_field_rel(to).to_string_lossy());
        assert_eq!(
            std::fs::read_dir(dir).unwrap().count(),
            1,
            "no file was copied"
        );
        assert_eq!(p.get(to), Some(old));
    }

    #[test]
    fn a_copy_that_fails_part_way_removes_what_it_copied_and_keeps_the_old_flat_field() {
        let p = Pair::new("flat-copy-cleanup");
        let (from, to) = (p.roll("From"), p.roll("To"));
        let old = p.flat(to, None, &["r-OLDOLDOL-old.NEF"]);
        let src = p.flat(from, None, &["r-AAAAAAAA-one.NEF", "g-AAAAAAAA-two.NEF"]);
        // The second file of the source is gone.
        std::fs::remove_file(p.path(&src.files[1])).unwrap();

        assert!(flat_field_copy(&p.lib, from, to).is_err());
        assert_eq!(p.get(to), Some(old.clone()), "unchanged");
        let names: Vec<String> = std::fs::read_dir(p.path(&flat_field_rel(to).to_string_lossy()))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["r-OLDOLDOL-old.NEF"], "the first copy was removed");
        assert!(p.path(&old.files[0]).exists());
    }

    #[test]
    fn a_copy_replaces_the_flat_field_of_the_target_and_removes_its_files() {
        let p = Pair::new("flat-copy-replace");
        let (from, to) = (p.roll("From"), p.roll("To"));
        let old = p.flat(to, None, &["r-OLDOLDOL-old.NEF"]);
        p.flat(from, None, &["r-AAAAAAAA-one.NEF"]);
        flat_field_copy(&p.lib, from, to).unwrap();
        assert!(
            !p.path(&old.files[0]).exists(),
            "the replaced file is removed"
        );
        let copy = p.get(to).unwrap();
        assert_eq!(copy.files.len(), 1);
        assert!(p.path(&copy.files[0]).exists());
    }

    #[test]
    fn deleting_the_source_roll_leaves_the_copy_and_its_files() {
        let p = Pair::new("flat-copy-delete");
        let (from, to) = (p.roll("From"), p.roll("To"));
        let src = p.flat(from, None, &["r-AAAAAAAA-one.NEF", "g-AAAAAAAA-two.NEF"]);
        flat_field_copy(&p.lib, from, to).unwrap();
        crate::services::roll_frame::roll_delete(&p.lib, from).unwrap();
        for rel in &src.files {
            assert!(!p.path(rel).exists(), "the source's files went with it");
        }
        let copy = p.get(to).unwrap();
        assert_eq!(copy.files.len(), 2);
        for rel in &copy.files {
            assert!(p.path(rel).exists(), "{rel}");
        }
    }

    #[test]
    fn the_list_has_every_flat_field_newest_first_ties_by_roll_name() {
        let p = Pair::new("flat-list");
        assert!(flat_field_list(&p.lib).unwrap().is_empty());
        let (a, b, c) = (p.roll("Beta"), p.roll("Alpha"), p.roll("Gamma"));
        p.roll("Without");
        p.flat(a, Some("2021-01-01T00:00:00Z"), &[]);
        p.flat(b, Some("2021-01-01T00:00:00Z"), &[]);
        p.flat(c, Some("2022-06-01T00:00:00Z"), &[]);
        let list = flat_field_list(&p.lib).unwrap();
        let names: Vec<&str> = list.iter().map(|s| s.roll_name.as_str()).collect();
        assert_eq!(names, ["Gamma", "Alpha", "Beta"]);
        assert_eq!(list[0].roll_id, c);
        assert_eq!(list[0].created_at, "2022-06-01T00:00:00Z");
        assert_eq!(list[0].exposure.iso, Some(100));
    }

    /// Hands out the mosaic of a bare-light exposure by the colour its kept name says (`r-`, `g-`,
    /// `b-`): a light that falls off towards the edges, with an opaque strip down the left side.
    struct BareLight;

    impl RawDecoder for BareLight {
        fn decode(&self, path: &Path, _: &Limits) -> Result<gtp_io::RawLinear, AppError> {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let lit = ["r-", "g-", "b-"]
                .iter()
                .position(|p| name.starts_with(p))
                .ok_or_else(|| AppError::new(ErrorCode::NotFound, "no such exposure"))?;
            let (w, h) = (120usize, 80usize);
            let mut data = Vec::with_capacity(w * h);
            for y in 0..h {
                for x in 0..w {
                    let value = if gtp_io::Cfa::RGGB.color_at(x, y) == lit {
                        let u = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
                        let v = (y as f32 + 0.5) / h as f32 * 2.0 - 1.0;
                        let open = if x >= 24 { 1.0 } else { 0.01 };
                        (1.0 - 0.3 * (u * u + v * v) / 2.0) * open * 9_000.0
                    } else {
                        120.0
                    };
                    data.push(value.round() as u16);
                }
            }
            Ok(gtp_io::RawLinear {
                width: w as u32,
                height: h as u32,
                cfa: gtp_io::Cfa::RGGB,
                black: [0.0; 4],
                white: 10_000.0,
                data,
            })
        }
    }

    const KEPT: [&str; 3] = [
        "r-AAAAAAAA-one.NEF",
        "g-AAAAAAAA-two.NEF",
        "b-AAAAAAAA-three.NEF",
    ];

    #[test]
    fn a_rebuild_makes_the_flat_field_again_from_its_kept_files_and_keeps_all_else() {
        let p = Pair::new("flat-rebuild");
        let roll = p.roll("Roll");
        let before = p.flat(roll, Some("2020-05-01T08:00:00Z"), &KEPT);
        let [r, g, b] = before.files.clone().try_into().unwrap();
        let merged = merge_triplet(
            &BareLight,
            &p.path(&r),
            &p.path(&g),
            &p.path(&b),
            &Limits::default(),
        )
        .unwrap()
        .image;
        let expected = build_flat_field(&merged, sources()).unwrap();
        assert_ne!(expected.light, before.light, "the stored one is another");

        let info = flat_field_rebuild_with(&p.lib, roll, &BareLight).unwrap();
        let after = p.get(roll).unwrap();
        assert_eq!(after.light, expected.light, "the current method");
        assert_eq!(
            (after.grid_w, after.grid_h),
            (expected.grid_w, expected.grid_h)
        );
        assert_eq!(after.sources, before.sources);
        assert_eq!(after.exposure, before.exposure);
        assert_eq!(after.files, before.files);
        assert_eq!(after.created_at, before.created_at);
        assert_eq!(info, FlatFieldInfo::from(&after));
        for rel in &after.files {
            assert!(p.path(rel).exists(), "{rel} is kept");
        }
        assert_eq!(
            std::fs::read_dir(p.path(&flat_field_rel(roll).to_string_lossy()))
                .unwrap()
                .count(),
            3
        );
        assert!(after.light.iter().all(|v| *v >= FlatField::MIN_LIGHT));
    }

    #[test]
    fn a_rebuild_that_cannot_be_made_is_refused_and_changes_nothing() {
        let p = Pair::new("flat-rebuild-refused");
        let (bare, not_kept, partly, gone) = (
            p.roll("Bare"),
            p.roll("Not kept"),
            p.roll("Partly"),
            p.roll("Gone"),
        );
        let code = |r: Result<FlatFieldInfo, AppError>| r.unwrap_err().code;

        assert_eq!(
            code(flat_field_rebuild_with(&p.lib, bare, &BareLight)),
            ErrorCode::InvalidInput,
            "no flat field"
        );
        assert_eq!(
            code(flat_field_rebuild_with(&p.lib, RollId::new(), &BareLight)),
            ErrorCode::NotFound
        );

        let old = p.flat(not_kept, None, &[]);
        let err = flat_field_rebuild_with(&p.lib, not_kept, &BareLight).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(err.message.contains("not kept"), "{}", err.message);
        assert_eq!(p.get(not_kept), Some(old));

        let old = p.flat(partly, None, &KEPT[..2]);
        assert_eq!(
            code(flat_field_rebuild_with(&p.lib, partly, &BareLight)),
            ErrorCode::InvalidInput,
            "two kept files"
        );
        assert_eq!(p.get(partly), Some(old));

        let old = p.flat(gone, None, &KEPT);
        std::fs::remove_file(p.path(&old.files[1])).unwrap();
        let err = flat_field_rebuild_with(&p.lib, gone, &BareLight).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(err.message.contains("gone"), "{}", err.message);
        assert_eq!(p.get(gone), Some(old.clone()), "unchanged");
        assert!(p.path(&old.files[0]).exists() && p.path(&old.files[2]).exists());
    }
}
