//! Importing existing 16-bit TIFF scans as frames (L-05). Per frame:
//!
//! 1. read the scan (and its infrared plane: embedded as a fourth channel, or a separate file),
//!    with the size limits applied before decoding;
//! 2. write `originals/<roll>/<frame>/scan.tif` (and `ir.tif`), Deflate with predictor;
//! 3. read them back and require identical samples, then record their SHA-256 and mark them
//!    read-only;
//! 4. analyze a proxy (the roll's film base when it has one) for the starting recipe: detected
//!    crop, film base and levels. The first frame analyzed on a roll with no base also stores its
//!    measured base, with the confidence, as the roll's base (`ensure_roll_base`), so every later
//!    frame uses the same one;
//!    A scanner frame also estimates its mid-tone neutral for the roll's colour balance (P-22): a
//!    roll that has one gives it to the frame at once, otherwise the median over the job is stored
//!    when the job ends and written into the job's unedited frames (`roll_balance`);
//! 5. render a 512 px sRGB thumbnail from that recipe;
//! 6. record frame, sources, recipe and search text in one catalog transaction, then announce the
//!    change.
//!
//! A file that cannot be imported is skipped with a typed reason and the rest go on. A frame that
//! fails or is cancelled part-way has its files removed, so no frame is recorded without its files,
//! and finished frames stay.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use gtp_core::{
    AppError, ErrorCode, Frame, FrameId, FrameKind, ImportResult, Look, Recipe, RollId, SkipReason,
    SkippedFile, SourceFile, SourceRole,
};
use gtp_edit::BalanceMethod;
use gtp_ingest::{scan_tiff_candidates, Rejected, TiffCandidate};
use gtp_io::{
    encode_jpeg, mark_read_only, read_tiff16, sha256_file, split_rgbi, to_f32_linear, write_tiff16,
    Compression, Limits, Tiff16,
};

use crate::jobs::{JobCtx, LibraryEvents};
use crate::library_fs::{originals_rel, thumb_rel};
use crate::paths::display_path;
use crate::pipeline::{
    analyze_with_balance, apply_balance_axes, frame_hint, render, LevelsPercentiles, PipelineInput,
    RenderTarget,
};
use crate::services::counters;
use crate::services::library::Library;
use crate::services::roll_balance::JobBalance;
use crate::services::roll_frame::{ensure_roll_base, record_frame_measurement, remove_tree};
use gtp_core::recipe::OutputSpace;

/// What kind of files an import brings in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ImportKind {
    /// 16-bit scans, with infrared embedded or as `*_ir.tif`.
    TiffScan,
    /// R, G and B exposures from a camera: NEF files, three to a frame, in channel order (H-06).
    RgbTriplet,
}

/// Long edge of a thumbnail, in pixels (spec section 5.1).
pub const THUMB_EDGE: u32 = 512;
/// JPEG quality of thumbnails.
pub const THUMB_QUALITY: u8 = 85;
/// The stage name reported while importing.
pub const STAGE: &str = "importing";

/// Steps of one frame, for progress within it.
pub(crate) const STEPS: f32 = 6.0;

/// What a skipped file becomes in the result.
pub(crate) fn skipped(path: &Path, reason: SkipReason, details: Option<String>) -> SkippedFile {
    SkippedFile {
        path: display_path(path),
        reason,
        details,
    }
}

/// Classifies an error reading a picked file.
fn read_failure(path: &Path, err: &AppError) -> SkippedFile {
    let reason = match err.code {
        ErrorCode::InvalidInput => SkipReason::TooLarge,
        ErrorCode::UnsupportedFormat => SkipReason::Unsupported,
        _ => SkipReason::Unreadable,
    };
    skipped(path, reason, Some(err.to_string()))
}

/// A frame that could not be imported: either skip it (and go on) or stop the job.
pub(crate) enum FrameError {
    Skip(SkippedFile),
    Stop(AppError),
}

impl From<AppError> for FrameError {
    fn from(err: AppError) -> Self {
        FrameError::Stop(err)
    }
}

/// The scan's RGB and infrared planes, checked.
fn read_planes(
    candidate: &TiffCandidate,
    limits: &Limits,
) -> Result<(Tiff16, Option<Tiff16>), FrameError> {
    let scan_path = &candidate.scan;
    let scan = read_tiff16(scan_path, limits)
        .map_err(|e| FrameError::Skip(read_failure(scan_path, &e)))?;
    let unsupported = |what: &str| {
        FrameError::Skip(skipped(
            scan_path,
            SkipReason::Unsupported,
            Some(what.to_owned()),
        ))
    };
    let (rgb, embedded_ir) = match scan.channels {
        3 => (scan, None),
        4 => {
            let (rgb, ir) =
                split_rgbi(&scan).map_err(|e| FrameError::Skip(read_failure(scan_path, &e)))?;
            (rgb, Some(ir))
        }
        n => {
            return Err(unsupported(&format!(
                "{n} channels; scans need RGB or RGB plus infrared"
            )))
        }
    };
    let separate_ir = match &candidate.ir {
        None => None,
        Some(ir_path) => {
            let ir = read_tiff16(ir_path, limits)
                .map_err(|e| FrameError::Skip(read_failure(ir_path, &e)))?;
            let fits = ir.channels == 1 && (ir.width, ir.height) == (rgb.width, rgb.height);
            if !fits {
                return Err(FrameError::Skip(skipped(
                    ir_path,
                    SkipReason::Unsupported,
                    Some(format!(
                        "infrared {}x{}x{} does not match the scan {}x{}",
                        ir.width, ir.height, ir.channels, rgb.width, rgb.height
                    )),
                )));
            }
            Some(ir)
        }
    };
    Ok((rgb, embedded_ir.or(separate_ir)))
}

/// Writes one original, checks it reads back identical, hashes it and marks it read-only.
pub(crate) fn store_original(
    library: &Library,
    rel: &Path,
    plane: &Tiff16,
    picked: &Path,
    role: SourceRole,
    frame: FrameId,
) -> Result<SourceFile, FrameError> {
    let path = library.resolve(rel)?;
    write_tiff16(&path, plane, Compression::DeflatePredictor)?;
    let back = read_tiff16(&path, &Limits::default())?;
    if back != *plane {
        return Err(FrameError::Skip(skipped(
            picked,
            SkipReason::VerifyFailed,
            Some(format!("{} read back different", display_path(&path))),
        )));
    }
    let sha256 = sha256_file(&path)?;
    let bytes = std::fs::metadata(&path)
        .map_err(|err| AppError::from_io(&err))?
        .len();
    mark_read_only(&path)?;
    Ok(SourceFile {
        frame_id: frame,
        role,
        rel_path: rel.to_string_lossy().replace('\\', "/"),
        sha256,
        bytes,
    })
}

/// The starting recipe of a new frame: analyzed on a proxy with the roll's film base. The first
/// frame of a roll with no base also stores its measured base for the whole roll (L-10), so every
/// later frame uses the same one. Returns the recipe and the pipeline input with that base.
///
/// A scanner frame (`kind` [`FrameKind::ScanRgbIr`]) takes the roll's colour balance into its colour
/// axes when the roll has one; when it has none, the frame's own mid-tone estimate is returned (the
/// axes stay 0 until the job ends, see `roll_balance`). A camera triplet gets neither.
pub(crate) fn analyse_for_import(
    library: &Library,
    roll: RollId,
    rgb: gtp_core::ImageF32,
    look: Look,
    kind: FrameKind,
) -> Result<(Recipe, PipelineInput, Option<[f32; 3]>), AppError> {
    // The roll as of now: an earlier frame of this import may have set its film base.
    let current = library.read(|c| c.get_roll(roll))?;
    let hint = frame_hint(&current);
    // A roll with no base, or only a guess, measures this frame's base: it is stored when it is
    // the first, or better than the guess (see `ensure_roll_base`).
    let measure = current.film_base_rgb.is_none() || current.film_base_uncertain();
    let mut input = PipelineInput {
        rgb: Arc::new(rgb),
        ir: None,
        film_base: current.film_base_rgb.filter(|_| !measure),
        full_long_edge: None,
    };
    let method = match (kind, current.color_balance) {
        (FrameKind::ScanRgbIr, None) => BalanceMethod::GreyWorld,
        _ => BalanceMethod::Off,
    };
    let analyse = |input: &PipelineInput| {
        analyze_with_balance(input, hint, look, LevelsPercentiles::DEFAULT, method)
    };
    let mut analysis = analyse(&input)?;
    if measure {
        let ensured = ensure_roll_base(library, roll, analysis.base)?;
        input.film_base = Some(ensured.rgb);
        if !ensured.stored_now {
            // The roll keeps the base it has (another import stored one, or the guess stands):
            // this frame uses that one.
            analysis = analyse(&input)?;
        }
    }
    if let Some(size) = analysis.frame.measured_size.filter(|_| hint.is_some()) {
        record_frame_measurement(library, roll, size)?;
    }
    let mut recipe = analysis.auto_recipe;
    match (kind, current.color_balance) {
        (FrameKind::ScanRgbIr, Some(axes)) => apply_balance_axes(&mut recipe, axes),
        // The roll's own correction is not known yet: keep this frame's estimate for it, not in
        // the recipe.
        _ => apply_balance_axes(&mut recipe, [0.0; 3]),
    }
    Ok((recipe, input, analysis.balance))
}

/// Says in `result` what the import did to the roll's film base: whether it is a guess now, and
/// whether a guess was replaced by a better measurement. `before` is the roll as the import found it.
pub(crate) fn report_roll_base(
    library: &Library,
    before: &gtp_core::Roll,
    result: &mut ImportResult,
) -> Result<(), AppError> {
    let after = library.read(|c| c.get_roll(before.id))?;
    result.base_is_guess = after.film_base_uncertain();
    result.base_replaced =
        before.film_base_uncertain() && after.film_base_rgb != before.film_base_rgb;
    Ok(())
}

/// Renders the 512 px sRGB thumbnail of a new frame from its starting recipe.
pub(crate) fn write_thumbnail(
    library: &Library,
    frame_id: FrameId,
    input: &PipelineInput,
    recipe: &Recipe,
) -> Result<(), AppError> {
    let thumb = render(
        input,
        recipe,
        RenderTarget::Preview {
            max_edge: THUMB_EDGE,
        },
    )?;
    encode_jpeg(
        &thumb,
        OutputSpace::Srgb,
        THUMB_QUALITY,
        &library.resolve(&thumb_rel(frame_id))?,
        None,
    )
}
/// Imports one candidate; on any failure the frame's files are removed by the caller.
fn import_one(
    library: &Library,
    roll: RollId,
    candidate: &TiffCandidate,
    frame_id: FrameId,
    ctx: &JobCtx,
    look: Look,
    progress: &dyn Fn(f32),
) -> Result<(Frame, Option<[f32; 3]>), FrameError> {
    let (rgb, ir) = read_planes(candidate, &Limits::default())?;
    let picked_ir = candidate.ir.as_deref();
    store_frame(
        library,
        roll,
        frame_id,
        &Planes {
            rgb: &rgb,
            ir: ir.as_ref(),
            picked_scan: &candidate.scan,
            picked_ir,
        },
        ctx,
        look,
        progress,
        None,
    )
}

/// The planes of a frame to store, and the files they came from (named in errors).
pub(crate) struct Planes<'a> {
    pub rgb: &'a Tiff16,
    pub ir: Option<&'a Tiff16>,
    pub picked_scan: &'a Path,
    pub picked_ir: Option<&'a Path>,
}

/// Stores one frame from its planes, as described in the module docs: originals, hash, starting
/// recipe, thumbnail and the catalog row. `scan` is the scanner frame number and a JSON record of
/// how it was scanned, for a frame made by the scanner (H-03); `None` for an import. `progress` is
/// told the step (1 to [`STEPS`]) done so far. Returns the frame and the colour balance estimated
/// for it (see [`analyse_for_import`]).
#[allow(clippy::too_many_arguments)]
pub(crate) fn store_frame(
    library: &Library,
    roll: RollId,
    frame_id: FrameId,
    planes: &Planes<'_>,
    ctx: &JobCtx,
    look: Look,
    progress: &dyn Fn(f32),
    scan: Option<(u32, &str)>,
) -> Result<(Frame, Option<[f32; 3]>), FrameError> {
    let (rgb, ir) = (planes.rgb, planes.ir);
    progress(1.0);
    ctx.check_cancelled()?;

    let dir_rel = originals_rel(roll, frame_id);
    std::fs::create_dir_all(library.resolve(&dir_rel)?).map_err(|err| AppError::from_io(&err))?;
    let mut sources = vec![store_original(
        library,
        &dir_rel.join("scan.tif"),
        rgb,
        planes.picked_scan,
        SourceRole::Rgb,
        frame_id,
    )?];
    progress(2.0);
    ctx.check_cancelled()?;
    if let Some(ir) = ir {
        let picked = planes.picked_ir.unwrap_or(planes.picked_scan);
        sources.push(store_original(
            library,
            &dir_rel.join("ir.tif"),
            ir,
            picked,
            SourceRole::Ir,
            frame_id,
        )?);
    }
    progress(3.0);
    ctx.check_cancelled()?;

    let (recipe, input, balance) = analyse_for_import(
        library,
        roll,
        to_f32_linear(rgb)?,
        look,
        FrameKind::ScanRgbIr,
    )?;
    progress(4.0);
    ctx.check_cancelled()?;

    write_thumbnail(library, frame_id, &input, &recipe)?;
    progress(5.0);
    ctx.check_cancelled()?;

    let frame = Frame {
        id: frame_id,
        roll_id: roll,
        index_in_roll: 0,
        kind: FrameKind::ScanRgbIr,
        width: rgb.width,
        height: rgb.height,
        has_ir: ir.is_some(),
        note: String::new(),
        flag: gtp_core::FrameFlag::None,
        created_at: String::new(),
        edited_at: None,
    };
    let stored = library.write(|c| match scan {
        Some((number, info)) => c.insert_scanned_frame(&frame, &sources, &recipe, number, info),
        None => c.insert_imported_frame(&frame, &sources, &recipe),
    })?;
    Ok((stored, balance))
}

/// Removes what a failed or cancelled frame left behind.
pub(crate) fn clean_up(library: &Library, roll: RollId, frame: FrameId) {
    let mut left = Vec::new();
    for rel in [originals_rel(roll, frame), thumb_rel(frame)] {
        if let Ok(path) = library.resolve(&rel) {
            remove_tree(&path, &mut left);
        }
    }
}

/// Imports `paths` into `roll` as described in the module docs. Ends with `Cancelled` when
/// cancelled (finished frames stay), or with the error that stopped it (a catalog or disk failure).
pub fn import_tiffs(
    library: &Library,
    roll: RollId,
    paths: &[PathBuf],
    ctx: &JobCtx,
    events: &dyn LibraryEvents,
) -> Result<ImportResult, AppError> {
    import_tiffs_with_look(library, roll, paths, ctx, events, Look::Neutral)
}

/// [`import_tiffs`] whose automatic recipes start from `look` (the Starting look setting).
pub fn import_tiffs_with_look(
    library: &Library,
    roll: RollId,
    paths: &[PathBuf],
    ctx: &JobCtx,
    events: &dyn LibraryEvents,
    look: Look,
) -> Result<ImportResult, AppError> {
    let before = library.read(|c| c.get_roll(roll))?;
    let grouped = scan_tiff_candidates(paths);
    let mut result = ImportResult {
        skipped: grouped
            .rejected
            .iter()
            .map(|r| match r {
                Rejected::NotTiff(p) => skipped(p, SkipReason::NotTiff, None),
                Rejected::OrphanIr(p) => skipped(p, SkipReason::OrphanIr, None),
            })
            .collect(),
        ..ImportResult::default()
    };
    let mut balance = JobBalance::default();
    let outcome = import_candidates(
        library,
        roll,
        &grouped.candidates,
        ctx,
        events,
        look,
        &mut result,
        &mut balance,
    );
    // The roll's colour balance is settled after a cancelled or failed job too, so the frames it
    // stored agree with the roll.
    let settled = balance.finish(library, roll);
    match outcome {
        Ok(()) => {
            settled?.report(&mut result);
            ctx.progress(STAGE, 1.0);
            report_roll_base(library, &before, &mut result)?;
            Ok(result)
        }
        Err(err) => {
            if let Ok(settled) = settled {
                settled.drop_thumbnails(library);
                if !settled.rebalanced.is_empty() {
                    events.library_changed(Some(roll));
                }
            }
            Err(err)
        }
    }
}

/// The frame loop of [`import_tiffs_with_look`]: stops with `Cancelled` or the error that stopped it,
/// leaving what was stored in `result` and `balance`.
#[allow(clippy::too_many_arguments)]
fn import_candidates(
    library: &Library,
    roll: RollId,
    candidates: &[TiffCandidate],
    ctx: &JobCtx,
    events: &dyn LibraryEvents,
    look: Look,
    result: &mut ImportResult,
    balance: &mut JobBalance,
) -> Result<(), AppError> {
    let total = candidates.len().max(1) as f32;
    for (i, candidate) in candidates.iter().enumerate() {
        ctx.check_cancelled()?;
        let started = Instant::now();
        let frame_id = FrameId::new();
        let progress = |step: f32| ctx.progress(STAGE, (i as f32 + step / STEPS) / total);
        progress(0.0);
        match import_one(library, roll, candidate, frame_id, ctx, look, &progress) {
            Ok((frame, estimate)) => {
                result.frames.push(frame.id);
                balance.record(frame.id, estimate);
                counters::count_import(library);
                result
                    .frame_ms
                    .push(u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX));
                events.library_changed(Some(roll));
            }
            Err(FrameError::Skip(file)) => {
                clean_up(library, roll, frame_id);
                result.skipped.push(file);
            }
            Err(FrameError::Stop(err)) => {
                clean_up(library, roll, frame_id);
                return Err(err);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use gtp_catalog::SCHEMA_VERSION;
    use gtp_core::testing::synthetic::{self, Params, Pattern};
    use gtp_core::{ImportWarning, ImportWarningKind};
    use gtp_io::from_f32_linear;

    use super::*;
    use crate::jobs::CancelFlag;
    use crate::library_fs::DB_FILE;

    struct Setup {
        dir: PathBuf,
        incoming: PathBuf,
        lib: Option<Library>,
        roll: RollId,
    }

    impl Setup {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "gtp-import-{label}-{}-{}",
                std::process::id(),
                FrameId::new()
            ));
            let lib = Library::create(&dir.join("lib"), "Imports").unwrap();
            let roll = crate::services::roll_frame::roll_create(
                &lib,
                crate::services::roll_frame::RollInput {
                    name: "Scans".into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .id;
            let incoming = dir.join("incoming");
            std::fs::create_dir_all(&incoming).unwrap();
            Self {
                dir,
                incoming,
                lib: Some(lib),
                roll,
            }
        }

        fn lib(&self) -> &Library {
            self.lib.as_ref().unwrap()
        }

        /// Writes a synthetic negative; `ir` adds the infrared plane as a 4th channel.
        fn scan(&self, name: &str, seed: u64, with_ir: bool) -> PathBuf {
            let s = synthetic::negative(
                seed,
                &Params {
                    width: 200,
                    height: 140,
                    rebate: [10; 4],
                    pattern: Pattern::Blobs,
                    with_ir,
                    dust_specks: if with_ir { 3 } else { 0 },
                    ..Params::default()
                },
            )
            .unwrap();
            let rgb = from_f32_linear(&s.image);
            let tiff = match &s.ir {
                Some(ir) => gtp_io::merge_rgbi(&rgb, &from_f32_linear(ir)).unwrap(),
                None => rgb,
            };
            let path = self.incoming.join(name);
            write_tiff16(&path, &tiff, Compression::None).unwrap();
            path
        }

        /// Writes a synthetic negative whose white point differs per channel (so each frame has a
        /// colour cast of its own), of `width` by `height` pixels.
        fn cast_scan(&self, name: &str, seed: u64, white: [f32; 3], size: (u32, u32)) -> PathBuf {
            let s = synthetic::negative(
                seed,
                &Params {
                    width: size.0,
                    height: size.1,
                    rebate: [if size.0 < 100 { 2 } else { 10 }; 4],
                    pattern: Pattern::Blobs,
                    white,
                    ..Params::default()
                },
            )
            .unwrap();
            let path = self.incoming.join(name);
            write_tiff16(&path, &from_f32_linear(&s.image), Compression::None).unwrap();
            path
        }

        fn new_roll(&self, name: &str) -> RollId {
            crate::services::roll_frame::roll_create(
                self.lib(),
                crate::services::roll_frame::RollInput {
                    name: name.into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .id
        }

        fn balance_of(&self, roll: RollId) -> Option<[f32; 3]> {
            self.lib().read(|c| c.get_roll(roll)).unwrap().color_balance
        }

        fn axes(&self, frame: FrameId) -> [f32; 3] {
            let r = self.lib().read(|c| c.load_recipe(frame)).unwrap().unwrap();
            [r.color.red_cyan, r.color.green_magenta, r.color.blue_yellow]
        }

        /// Writes a separate 1-channel infrared file of the given size.
        fn ir_file(&self, name: &str, width: u32, height: u32) -> PathBuf {
            let ir = Tiff16 {
                width,
                height,
                channels: 1,
                data: vec![60_000; (width * height) as usize],
            };
            let path = self.incoming.join(name);
            write_tiff16(&path, &ir, Compression::None).unwrap();
            path
        }
    }

    impl Drop for Setup {
        fn drop(&mut self) {
            self.lib = None;
            let mut left = Vec::new();
            remove_tree(&self.dir, &mut left);
        }
    }

    #[derive(Default)]
    struct Events {
        changed: Mutex<u32>,
        cancel_after: Option<(u32, CancelFlag)>,
    }

    impl LibraryEvents for Events {
        fn library_changed(&self, _: Option<RollId>) {
            let mut n = self.changed.lock().unwrap();
            *n += 1;
            if let Some((after, flag)) = &self.cancel_after {
                if *n >= *after {
                    flag.cancel();
                }
            }
        }
    }

    fn files_under(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| {
                        let p = e.path();
                        if p.is_dir() {
                            files_under(&p)
                        } else {
                            1
                        }
                    })
                    .sum()
            })
            .unwrap_or(0)
    }

    const SEEDS: [u64; 3] = [1, 2, 4];
    const CASTS: [[f32; 3]; 3] = [[2.1, 2.3, 2.5], [2.2, 2.3, 2.4], [2.15, 2.3, 2.45]];

    fn import_casts(s: &Setup, roll: RollId, names: &[&str]) -> ImportResult {
        let paths: Vec<PathBuf> = names
            .iter()
            .enumerate()
            .map(|(i, n)| s.cast_scan(n, SEEDS[i % 3], CASTS[i % 3], (200, 140)))
            .collect();
        let (ctx, _) = JobCtx::standalone();
        import_tiffs(s.lib(), roll, &paths, &ctx, &Events::default()).unwrap()
    }

    #[test]
    fn a_scan_import_stores_the_median_balance_on_the_roll_and_writes_it_into_every_frame() {
        let s = Setup::new("balance");
        let result = import_casts(&s, s.roll, &["a.tif", "b.tif", "c.tif"]);
        assert_eq!(result.frames.len(), 3);
        assert!(
            result.warnings.is_empty(),
            "every frame was judged: {:?}",
            result.warnings
        );

        // Each frame alone on a roll of its own: its balance is its own estimate.
        let own: Vec<[f32; 3]> = ["a.tif", "b.tif", "c.tif"]
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let roll = s.new_roll(name);
                let path = s.cast_scan(name, SEEDS[i], CASTS[i], (200, 140));
                let (ctx, _) = JobCtx::standalone();
                import_tiffs(s.lib(), roll, &[path], &ctx, &Events::default()).unwrap();
                s.balance_of(roll).expect("a single frame is judged")
            })
            .collect();
        assert!(
            own[0] != own[1] && own[1] != own[2],
            "casts differ: {own:?}"
        );

        let median = gtp_edit::roll_balance(&own).unwrap();
        assert_eq!(s.balance_of(s.roll), Some(median));
        for id in &result.frames {
            assert_eq!(s.axes(*id), median);
            let frame = s.lib().read(|c| c.get_frame(*id)).unwrap();
            assert!(frame.edited_at.is_none());
        }
        assert_eq!(
            result.rebalanced.len(),
            3,
            "their thumbnails are made again"
        );
    }

    #[test]
    fn a_second_import_into_a_roll_with_a_balance_uses_it_directly() {
        let s = Setup::new("balance-second");
        import_casts(&s, s.roll, &["a.tif", "b.tif"]);
        let stored = s.balance_of(s.roll).unwrap();
        let again = import_casts(&s, s.roll, &["c.tif"]);
        assert_eq!(s.axes(again.frames[0]), stored);
        assert_eq!(s.balance_of(s.roll), Some(stored), "the first stays");
        assert!(again.rebalanced.is_empty(), "it had it from the start");
        assert!(again.warnings.is_empty());
    }

    #[test]
    fn a_job_with_no_judgeable_frame_stores_no_balance_and_says_so() {
        let s = Setup::new("balance-none");
        // 30 by 20 pixels, a frame of fewer pixels than an estimate needs.
        let tiny = s.cast_scan("tiny.tif", 1, CASTS[0], (30, 20));
        let (ctx, _) = JobCtx::standalone();
        let result = import_tiffs(s.lib(), s.roll, &[tiny], &ctx, &Events::default()).unwrap();
        assert_eq!(result.frames.len(), 1);
        assert_eq!(s.balance_of(s.roll), None);
        assert_eq!(s.axes(result.frames[0]), [0.0; 3]);
        assert_eq!(
            result.warnings,
            vec![ImportWarning {
                frame_id: result.frames[0],
                kind: ImportWarningKind::BalanceSkipped
            }]
        );
        assert!(result.rebalanced.is_empty());
    }

    #[test]
    fn a_frame_that_cannot_be_judged_still_gets_the_rolls_balance() {
        let s = Setup::new("balance-unjudged");
        let mut paths = vec![
            s.cast_scan("a.tif", SEEDS[0], CASTS[0], (200, 140)),
            s.cast_scan("tiny.tif", 2, CASTS[1], (30, 20)),
            s.cast_scan("c.tif", SEEDS[2], CASTS[2], (200, 140)),
        ];
        paths.sort();
        let (ctx, _) = JobCtx::standalone();
        let result = import_tiffs(s.lib(), s.roll, &paths, &ctx, &Events::default()).unwrap();
        let balance = s.balance_of(s.roll).expect("two frames were judged");
        assert_eq!(result.frames.len(), 3);
        for id in &result.frames {
            assert_eq!(s.axes(*id), balance);
        }
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn five_scans_become_frames_with_verified_read_only_originals_and_thumbnails() {
        let s = Setup::new("five");
        let mut paths = vec![
            s.scan("f1.tif", 1, false),
            s.scan("f2.tif", 2, true),
            s.scan("f3.tiff", 3, false),
            s.scan("f4.tif", 4, false),
            s.scan("f5.TIF", 5, false),
        ];
        paths.push(s.ir_file("f1_IR.tiff", 200, 140));
        let (ctx, _) = JobCtx::standalone();
        let events = Events::default();
        let result = import_tiffs(s.lib(), s.roll, &paths, &ctx, &events).unwrap();
        assert_eq!(result.frames.len(), 5, "{:?}", result.skipped);
        assert!(result.skipped.is_empty());
        assert_eq!(result.frame_ms.len(), 5);
        assert_eq!(*events.changed.lock().unwrap(), 5);
        let counted = counters::read(s.lib()).unwrap();
        assert_eq!(counted.frames_imported, 5);
        assert!(counted.first_import_at.is_some());

        for (i, id) in result.frames.iter().enumerate() {
            let frame = s.lib().read(|c| c.get_frame(*id)).unwrap();
            assert_eq!(frame.index_in_roll, i as u32 + 1);
            assert_eq!(
                frame.has_ir,
                i < 2,
                "f1 (separate IR) and f2 (RGBI) have infrared"
            );
            assert!(frame.edited_at.is_none());
            let sources = s.lib().read(|c| c.sources_of(*id)).unwrap();
            assert_eq!(sources.len(), if i < 2 { 2 } else { 1 });
            for source in sources {
                let path = s.lib().resolve(Path::new(&source.rel_path)).unwrap();
                assert_eq!(sha256_file(&path).unwrap(), source.sha256);
                assert_eq!(std::fs::metadata(&path).unwrap().len(), source.bytes);
                assert!(std::fs::metadata(&path).unwrap().permissions().readonly());
            }
            let thumb = s.lib().resolve(&thumb_rel(*id)).unwrap();
            assert!(std::fs::read(&thumb).unwrap().starts_with(&[0xFF, 0xD8]));
            let recipe = s.lib().read(|c| c.load_recipe(*id)).unwrap().unwrap();
            assert_ne!(
                recipe,
                Recipe::default(),
                "the auto recipe is the starting point"
            );
            assert!(recipe.geometry.crop.w < 1.0, "the rebate is cropped away");
        }
        // The original pixels are the picked file's pixels.
        let first = s.lib().read(|c| c.sources_of(result.frames[0])).unwrap();
        let rgb = first.iter().find(|s| s.role == SourceRole::Rgb).unwrap();
        let stored = read_tiff16(
            &s.lib().resolve(Path::new(&rgb.rel_path)).unwrap(),
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(stored, read_tiff16(&paths[0], &Limits::default()).unwrap());

        // A second import continues the numbering.
        let more = import_tiffs(
            s.lib(),
            s.roll,
            &[s.scan("f6.tif", 6, false)],
            &ctx,
            &events,
        )
        .unwrap();
        let frame = s.lib().read(|c| c.get_frame(more.frames[0])).unwrap();
        assert_eq!(frame.index_in_roll, 6);
    }

    #[test]
    fn the_first_frame_sets_the_roll_base_and_every_later_frame_shares_it() {
        let s = Setup::new("roll-base");
        let (ctx, _) = JobCtx::standalone();
        let events = Events::default();
        let first = import_tiffs(
            s.lib(),
            s.roll,
            &[s.scan("a.tif", 1, false), s.scan("b.tif", 2, false)],
            &ctx,
            &events,
        )
        .unwrap();
        assert_eq!(first.frames.len(), 2);
        let roll = s.lib().read(|c| c.get_roll(s.roll)).unwrap();
        let base = roll.film_base_rgb.unwrap();
        assert!(roll.film_base_confidence.is_some_and(|c| c > 0.5));
        assert!(!roll.film_base_uncertain());
        for (c, truth) in [0.80f32, 0.55, 0.35].into_iter().enumerate() {
            assert!(
                (base[c] - truth).abs() / truth < 0.02,
                "channel {c}: {base:?}"
            );
        }
        // Both frames' recipes carry the roll's base, not a base of their own.
        for id in &first.frames {
            let recipe = s.lib().read(|c| c.load_recipe(*id)).unwrap().unwrap();
            assert_eq!(recipe.base.rgb, base);
        }
        // A later import into the roll uses it and never overwrites it.
        let later =
            import_tiffs(s.lib(), s.roll, &[s.scan("c.tif", 3, false)], &ctx, &events).unwrap();
        let recipe = s
            .lib()
            .read(|c| c.load_recipe(later.frames[0]))
            .unwrap()
            .unwrap();
        assert_eq!(recipe.base.rgb, base);
        assert_eq!(
            s.lib().read(|c| c.get_roll(s.roll)).unwrap().film_base_rgb,
            Some(base)
        );
    }

    #[test]
    fn a_base_set_by_hand_is_used_and_left_alone() {
        let s = Setup::new("manual-base");
        let manual = [0.7, 0.5, 0.3];
        crate::services::roll_frame::roll_set_film_base(s.lib(), s.roll, Some(manual)).unwrap();
        let (ctx, _) = JobCtx::standalone();
        let result = import_tiffs(
            s.lib(),
            s.roll,
            &[s.scan("a.tif", 1, false)],
            &ctx,
            &Events::default(),
        )
        .unwrap();
        let roll = s.lib().read(|c| c.get_roll(s.roll)).unwrap();
        assert_eq!(roll.film_base_rgb, Some(manual));
        assert_eq!(roll.film_base_confidence, None);
        let recipe = s
            .lib()
            .read(|c| c.load_recipe(result.frames[0]))
            .unwrap()
            .unwrap();
        assert_eq!(recipe.base.rgb, manual);
    }

    #[test]
    fn a_frame_without_rebate_gives_a_flagged_roll_base() {
        let s = Setup::new("no-rebate");
        let img = synthetic::negative(
            4,
            &Params {
                width: 200,
                height: 140,
                rebate: [0; 4],
                pattern: Pattern::Gradient,
                ..Params::default()
            },
        )
        .unwrap();
        let path = s.incoming.join("tight.tif");
        write_tiff16(&path, &from_f32_linear(&img.image), Compression::None).unwrap();
        let (ctx, _) = JobCtx::standalone();
        let result = import_tiffs(s.lib(), s.roll, &[path], &ctx, &Events::default()).unwrap();
        assert_eq!(result.frames.len(), 1, "the frame is still imported");
        let roll = s.lib().read(|c| c.get_roll(s.roll)).unwrap();
        assert!(roll.film_base_rgb.is_some());
        assert!(
            roll.film_base_uncertain(),
            "confidence {:?}",
            roll.film_base_confidence
        );
    }

    #[test]
    fn a_guessed_roll_base_is_reported_and_a_confident_frame_replaces_it() {
        let s = Setup::new("guess-replaced");
        let tight = synthetic::negative(
            4,
            &Params {
                width: 200,
                height: 140,
                rebate: [0; 4],
                pattern: Pattern::Gradient,
                ..Params::default()
            },
        )
        .unwrap();
        let tight_path = s.incoming.join("tight.tif");
        write_tiff16(
            &tight_path,
            &from_f32_linear(&tight.image),
            Compression::None,
        )
        .unwrap();
        let (ctx, _) = JobCtx::standalone();
        let first = import_tiffs(s.lib(), s.roll, &[tight_path], &ctx, &Events::default()).unwrap();
        assert!(first.base_is_guess, "the roll's base is a guess");
        assert!(!first.base_replaced);
        let guess = s.lib().read(|c| c.get_roll(s.roll)).unwrap().film_base_rgb;

        // A frame with a clear rebate gives a confident base, which replaces the guess.
        let good = s.scan("good.tif", 1, false);
        let second = import_tiffs(s.lib(), s.roll, &[good], &ctx, &Events::default()).unwrap();
        assert!(second.base_replaced && !second.base_is_guess);
        let roll = s.lib().read(|c| c.get_roll(s.roll)).unwrap();
        assert!(!roll.film_base_uncertain());
        assert_ne!(roll.film_base_rgb, guess);

        // From now on the roll's base stays, and a later import says nothing about it.
        let more = s.scan("more.tif", 2, false);
        let third = import_tiffs(s.lib(), s.roll, &[more], &ctx, &Events::default()).unwrap();
        assert!(!third.base_replaced && !third.base_is_guess);
        assert_eq!(
            s.lib().read(|c| c.get_roll(s.roll)).unwrap().film_base_rgb,
            roll.film_base_rgb
        );
    }

    #[test]
    fn the_starting_look_is_stored_in_the_recipe_of_a_new_frame() {
        let s = Setup::new("look");
        let (ctx, _) = JobCtx::standalone();
        let neutral = import_tiffs(
            s.lib(),
            s.roll,
            &[s.scan("a.tif", 1, false)],
            &ctx,
            &Events::default(),
        )
        .unwrap();
        let looked = import_tiffs_with_look(
            s.lib(),
            s.roll,
            &[s.scan("b.tif", 2, false)],
            &ctx,
            &Events::default(),
            Look::FilmLog,
        )
        .unwrap();
        let master = |id| {
            s.lib()
                .read(|c| c.load_recipe(id))
                .unwrap()
                .unwrap()
                .tone
                .curve
                .master
        };
        assert_eq!(
            master(neutral.frames[0]),
            Recipe::default().tone.curve.master,
            "Neutral is the default"
        );
        assert_eq!(master(looked.frames[0]).len(), 16);
    }

    #[test]
    fn a_base_picked_by_hand_is_never_called_a_guess() {
        let s = Setup::new("hand-not-guess");
        crate::services::roll_frame::roll_set_film_base(s.lib(), s.roll, Some([0.7, 0.4, 0.2]))
            .unwrap();
        let (ctx, _) = JobCtx::standalone();
        let good = s.scan("good.tif", 1, false);
        let result = import_tiffs(s.lib(), s.roll, &[good], &ctx, &Events::default()).unwrap();
        assert!(!result.base_is_guess && !result.base_replaced);
        assert_eq!(
            s.lib().read(|c| c.get_roll(s.roll)).unwrap().film_base_rgb,
            Some([0.7, 0.4, 0.2])
        );
    }

    #[test]
    fn bad_files_are_skipped_with_their_reasons() {
        let s = Setup::new("skips");
        let good = s.scan("good.tif", 1, false);
        let corrupt = s.incoming.join("corrupt.tif");
        std::fs::write(&corrupt, b"II*\0 not really").unwrap();
        let eight_bit = s.incoming.join("gray.tif");
        write_tiff16(
            &eight_bit,
            &Tiff16 {
                width: 8,
                height: 8,
                channels: 1,
                data: vec![1; 64],
            },
            Compression::None,
        )
        .unwrap();
        let missing = s.incoming.join("gone.tif");
        let mismatched = s.scan("pair.tif", 2, false);
        let wrong_ir = s.ir_file("pair_ir.tif", 10, 10);
        let paths = vec![
            good,
            corrupt,
            eight_bit,
            missing,
            mismatched,
            wrong_ir,
            s.incoming.join("notes.txt"),
            s.ir_file("lonely_ir.tif", 4, 4),
        ];
        let (ctx, _) = JobCtx::standalone();
        let result = import_tiffs(s.lib(), s.roll, &paths, &ctx, &Events::default()).unwrap();
        assert_eq!(result.frames.len(), 1);
        let reasons: Vec<(String, SkipReason)> = result
            .skipped
            .iter()
            .map(|f| {
                let name = Path::new(&f.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                (name, f.reason)
            })
            .collect();
        for expected in [
            ("notes.txt", SkipReason::NotTiff),
            ("lonely_ir.tif", SkipReason::OrphanIr),
            ("corrupt.tif", SkipReason::Unsupported),
            ("gone.tif", SkipReason::Unreadable),
            ("gray.tif", SkipReason::Unsupported),
            ("pair_ir.tif", SkipReason::Unsupported),
        ] {
            assert!(
                reasons.contains(&(expected.0.to_owned(), expected.1)),
                "{expected:?} missing from {reasons:?}"
            );
        }
        // Only the good frame left files behind.
        let originals = s.lib().root().join("originals");
        assert_eq!(files_under(&originals), 1);
        assert_eq!(files_under(&s.lib().root().join("thumbs")), 1);
    }

    #[test]
    fn an_oversized_scan_is_skipped_as_too_large() {
        let s = Setup::new("large");
        let path = s.scan("big.tif", 1, false);
        let tiny = Limits {
            max_side: 50,
            max_decoded_bytes: u64::MAX,
        };
        let candidate = TiffCandidate {
            scan: path,
            ir: None,
        };
        match read_planes(&candidate, &tiny) {
            Err(FrameError::Skip(file)) => assert_eq!(file.reason, SkipReason::TooLarge),
            _ => unreachable!("a scan over the limit must be skipped"),
        }
    }

    #[test]
    fn cancel_keeps_finished_frames_only() {
        let s = Setup::new("cancel");
        let paths: Vec<PathBuf> = (1..=4)
            .map(|i| s.scan(&format!("c{i}.tif"), i, false))
            .collect();
        let (ctx, flag) = JobCtx::standalone();
        let events = Events {
            cancel_after: Some((2, flag)),
            ..Events::default()
        };
        let err = import_tiffs(s.lib(), s.roll, &paths, &ctx, &events).unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        assert_eq!(
            s.lib()
                .read(|c| c.count_frames(Some(s.roll), None, false))
                .unwrap(),
            2
        );
        let originals = s.lib().root().join("originals").join(s.roll.to_string());
        let frame_dirs = std::fs::read_dir(&originals).unwrap().count();
        assert_eq!(frame_dirs, 2, "no folder of an unfinished frame is left");
        assert_eq!(
            counters::read(s.lib()).unwrap().frames_imported,
            2,
            "the frames that finished are counted even though the job was cancelled"
        );
        // The unfinished frame left no thumbnail. The two finished frames got the roll's colour
        // balance when the job ended, so their thumbnails were dropped to be rebuilt from the
        // saved recipe when asked for (nothing stale is left).
        assert_eq!(files_under(&s.lib().root().join("thumbs")), 0);
        let balance = s.balance_of(s.roll).expect("the two frames were judged");
        let frames = s.lib().read(|c| c.list_frames(s.roll)).unwrap();
        let develop = crate::services::develop::DevelopService::default();
        let thumbnails = crate::services::thumbnails::ThumbnailService::default();
        for frame in frames {
            assert_eq!(s.axes(frame.id), balance);
            let bytes = thumbnails.thumbnail(&develop, s.lib(), frame.id).unwrap();
            assert!(bytes.starts_with(&[0xFF, 0xD8]));
        }
        assert_eq!(files_under(&s.lib().root().join("thumbs")), 2);
    }

    #[test]
    fn a_failed_catalog_write_removes_the_frames_files_and_stops() {
        let mut s = Setup::new("fails");
        let path = s.scan("x.tif", 1, false);
        // A newer app version wrote the library: it reopens read-only and every write fails.
        s.lib = None;
        let db = s.dir.join("lib").join(DB_FILE);
        {
            use std::io::{Seek, SeekFrom, Write};
            let mut file = std::fs::OpenOptions::new().write(true).open(&db).unwrap();
            file.seek(SeekFrom::Start(60)).unwrap();
            file.write_all(&(SCHEMA_VERSION + 1).to_be_bytes()).unwrap();
        }
        s.lib = Some(Library::open(&s.dir.join("lib")).unwrap());
        let (ctx, _) = JobCtx::standalone();
        let err = import_tiffs(s.lib(), s.roll, &[path], &ctx, &Events::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::LibraryTooNew);
        assert_eq!(files_under(&s.lib().root().join("originals")), 0);
        assert_eq!(files_under(&s.lib().root().join("thumbs")), 0);
    }

    #[test]
    fn importing_into_a_missing_roll_is_not_found() {
        let s = Setup::new("noroll");
        let (ctx, _) = JobCtx::standalone();
        let err = import_tiffs(s.lib(), RollId::new(), &[], &ctx, &Events::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    /// One 21 MP frame against the 8 s budget; run with
    /// `cargo test -p gtp-app --release -- --ignored --nocapture time_import`.
    #[test]
    #[ignore = "timing run, not a check"]
    fn time_import_of_a_21_megapixel_frame() {
        let s = Setup::new("timing");
        let small = synthetic::negative(
            9,
            &Params {
                width: 2808,
                height: 1872,
                rebate: [60; 4],
                pattern: Pattern::Blobs,
                ..Params::default()
            },
        )
        .unwrap();
        // Doubled in both directions to 5616 x 3744 (the generator stops at 4096 px a side).
        let (w, h) = (small.image.width as usize, small.image.height as usize);
        let mut data = Vec::with_capacity(w * h * 12);
        for y in 0..h * 2 {
            for x in 0..w * 2 {
                let at = ((y / 2) * w + x / 2) * 3;
                data.extend_from_slice(&small.image.data[at..at + 3]);
            }
        }
        let big = gtp_core::ImageF32::new(w as u32 * 2, h as u32 * 2, 3, data).unwrap();
        let path = s.incoming.join("big.tif");
        write_tiff16(&path, &from_f32_linear(&big), Compression::None).unwrap();
        let (ctx, _) = JobCtx::standalone();
        let result = import_tiffs(s.lib(), s.roll, &[path], &ctx, &Events::default()).unwrap();
        eprintln!(
            "21 MP frame imported in {} ms (budget 8000)",
            result.frame_ms[0]
        );
    }
}
