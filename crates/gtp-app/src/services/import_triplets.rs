//! Importing RGB triplets from a camera (H-06). Per frame, three NEF files in channel order (red-,
//! green-, blue-lit):
//!
//! 1. merge them (`gtp_ingest::merge_triplet`: decoded side by side, only the lit channel of each
//!    taken), which also gives the warnings;
//! 2. write `originals/<roll>/<frame>/{r,g,b}.tif`, each the lit channel as a single-channel 16-bit
//!    TIFF (Deflate with predictor), read back and compared, hashed and marked read-only, the
//!    three side by side so that they fit the 10 s budget;
//! 3. analyze the merged picture for the starting recipe (the first frame of a roll with no film
//!    base measures it for the whole roll), render the thumbnail;
//! 4. record the frame (`RgbTriplet`), its three sources, the recipe and the search text in one
//!    catalog transaction, then announce the change.
//!
//! The NEF files themselves are not copied: the library keeps the converted planes (specification
//! section 5.1). A triplet that cannot be imported is skipped, its three files listed with the
//! reason, and the rest go on. A frame that fails or is cancelled part-way has its files removed;
//! finished frames stay.

use std::path::PathBuf;
use std::time::Instant;

use gtp_core::{
    AppError, ErrorCode, Frame, FrameId, FrameKind, ImageF32, ImportResult, ImportWarning,
    ImportWarningKind, RollId, SkipReason, SkippedFile, SourceFile, SourceRole,
};
use gtp_ingest::{merge_triplet, TripletWarning};
use gtp_io::{from_f32_linear, Limits, RawDecoder, Tiff16};

use super::import::{
    analyse_for_import, clean_up, skipped, store_original, write_thumbnail, FrameError, STAGE,
    STEPS,
};
use crate::jobs::{JobCtx, LibraryEvents};
use crate::library_fs::originals_rel;
use crate::services::counters;
use crate::services::library::Library;

/// A triplet that was not imported, or a failure that stops the whole job.
enum TripletError {
    Skip(Vec<SkippedFile>),
    Stop(AppError),
}

impl From<AppError> for TripletError {
    fn from(err: AppError) -> Self {
        TripletError::Stop(err)
    }
}

impl From<FrameError> for TripletError {
    fn from(err: FrameError) -> Self {
        match err {
            FrameError::Skip(file) => TripletError::Skip(vec![file]),
            FrameError::Stop(err) => TripletError::Stop(err),
        }
    }
}

/// What a failure to decode or merge the three files means: skip the triplet, or stop.
fn merge_failure(files: &[PathBuf], err: &AppError) -> TripletError {
    let reason = match err.code {
        ErrorCode::NotFound | ErrorCode::Io => SkipReason::Unreadable,
        ErrorCode::UnsupportedFormat => SkipReason::Unsupported,
        ErrorCode::InvalidInput if err.message.contains("larger than") => SkipReason::TooLarge,
        ErrorCode::InvalidInput => SkipReason::Unsupported,
        _ => return TripletError::Stop(err.clone()),
    };
    TripletError::Skip(
        files
            .iter()
            .map(|file| skipped(file, reason, Some(err.to_string())))
            .collect(),
    )
}

/// The warning as the job result carries it.
fn warning_kind(warning: &TripletWarning) -> ImportWarningKind {
    match warning {
        TripletWarning::ExposureTimeMismatch => ImportWarningKind::ExposureTimeMismatch,
        TripletWarning::IsoMismatch => ImportWarningKind::IsoMismatch,
        TripletWarning::ApertureMismatch => ImportWarningKind::ApertureMismatch,
        TripletWarning::Clipped { channel, fraction } => ImportWarningKind::Clipped {
            channel: *channel,
            per_mille: (fraction * 1000.0).round() as u32,
        },
    }
}

/// One channel of a 3-channel picture as a single-channel 16-bit plane.
fn plane(image: &ImageF32, channel: usize) -> Result<Tiff16, AppError> {
    let data = image
        .data
        .iter()
        .skip(channel)
        .step_by(3)
        .copied()
        .collect();
    Ok(from_f32_linear(&ImageF32::new(
        image.width,
        image.height,
        1,
        data,
    )?))
}

/// Imports one triplet; on any failure the frame's files are removed by the caller.
fn import_one(
    library: &Library,
    roll: RollId,
    files: &[PathBuf],
    frame_id: FrameId,
    decoder: &dyn RawDecoder,
    ctx: &JobCtx,
    progress: &dyn Fn(f32),
) -> Result<(Frame, Vec<ImportWarningKind>), TripletError> {
    let merged = merge_triplet(decoder, &files[0], &files[1], &files[2], &Limits::default())
        .map_err(|err| merge_failure(files, &err))?;
    progress(1.0);
    ctx.check_cancelled()?;

    let (width, height) = (merged.image.width, merged.image.height);
    let planes = [
        plane(&merged.image, 0)?,
        plane(&merged.image, 1)?,
        plane(&merged.image, 2)?,
    ];
    let dir_rel = originals_rel(roll, frame_id);
    std::fs::create_dir_all(library.resolve(&dir_rel)?).map_err(|err| AppError::from_io(&err))?;
    let names = ["r.tif", "g.tif", "b.tif"];
    let roles = [SourceRole::R, SourceRole::G, SourceRole::B];
    // The three planes side by side: each is written, read back, hashed and marked read-only.
    let stored: Vec<Result<SourceFile, FrameError>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..3)
            .map(|i| {
                let (planes, files, dir_rel) = (&planes, files, &dir_rel);
                scope.spawn(move || {
                    store_original(
                        library,
                        &dir_rel.join(names[i]),
                        &planes[i],
                        &files[i],
                        roles[i],
                        frame_id,
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle.join().unwrap_or_else(|_| {
                    Err(FrameError::Stop(AppError::internal(
                        "a triplet store thread panicked",
                    )))
                })
            })
            .collect()
    });
    let mut sources = Vec::with_capacity(3);
    for result in stored {
        sources.push(result?);
    }
    progress(3.0);
    ctx.check_cancelled()?;

    let warnings: Vec<ImportWarningKind> = merged.warnings.iter().map(warning_kind).collect();
    let (recipe, input) = analyse_for_import(library, roll, merged.image)?;
    progress(4.0);
    ctx.check_cancelled()?;

    write_thumbnail(library, frame_id, &input, &recipe)?;
    progress(5.0);
    ctx.check_cancelled()?;

    let frame = Frame {
        id: frame_id,
        roll_id: roll,
        index_in_roll: 0,
        kind: FrameKind::RgbTriplet,
        width,
        height,
        has_ir: false,
        note: String::new(),
        created_at: String::new(),
        edited_at: None,
    };
    library.write(|c| c.insert_imported_frame(&frame, &sources, &recipe))?;
    Ok((frame, warnings))
}

/// Imports the NEF files in `paths` into `roll`: three to a frame, in channel order (red-, green-,
/// blue-lit), as described in the module docs. `decoder` reads the camera files. Ends with
/// `Cancelled` when cancelled (finished frames stay), or with the error that stopped it (a catalog
/// or disk failure).
pub fn import_triplets(
    library: &Library,
    roll: RollId,
    paths: &[PathBuf],
    decoder: &dyn RawDecoder,
    ctx: &JobCtx,
    events: &dyn LibraryEvents,
) -> Result<ImportResult, AppError> {
    library.read(|c| c.get_roll(roll))?;
    if paths.is_empty() || !paths.len().is_multiple_of(3) {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "A triplet is three files: the red-, green- and blue-lit exposure.",
        )
        .with_details(format!("{} files", paths.len())));
    }
    let mut result = ImportResult::default();
    let total = (paths.len() / 3) as f32;
    for (i, files) in paths.chunks(3).enumerate() {
        ctx.check_cancelled()?;
        let started = Instant::now();
        let frame_id = FrameId::new();
        let progress = |step: f32| ctx.progress(STAGE, (i as f32 + step / STEPS) / total);
        progress(0.0);
        match import_one(library, roll, files, frame_id, decoder, ctx, &progress) {
            Ok((frame, warnings)) => {
                result.frames.push(frame.id);
                result
                    .warnings
                    .extend(warnings.into_iter().map(|kind| ImportWarning {
                        frame_id: frame.id,
                        kind,
                    }));
                counters::count_import(library);
                result
                    .frame_ms
                    .push(u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX));
                events.library_changed(Some(roll));
            }
            Err(TripletError::Skip(files)) => {
                clean_up(library, roll, frame_id);
                result.skipped.extend(files);
            }
            Err(TripletError::Stop(err)) => {
                clean_up(library, roll, frame_id);
                return Err(err);
            }
        }
    }
    ctx.progress(STAGE, 1.0);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    use gtp_core::testing::synthetic::{self, Params, Pattern};
    use gtp_core::SourceRole;
    use gtp_io::{demosaic_channel, read_tiff16, Cfa, RawLinear};

    use super::*;
    use crate::jobs::CancelFlag;
    use crate::services::develop::{read_scan, DevelopService};
    use crate::services::roll_frame::{roll_create, RollInput};

    const W: u32 = 200;
    const H: u32 = 140;

    /// Hands out constructed mosaics by file name, or the error stored for it.
    struct Fake(HashMap<PathBuf, Result<RawLinear, AppError>>);

    impl RawDecoder for Fake {
        fn decode(&self, path: &Path, _: &Limits) -> Result<RawLinear, AppError> {
            self.0
                .get(path)
                .cloned()
                .unwrap_or_else(|| Err(AppError::new(ErrorCode::NotFound, "no such file")))
        }
    }

    /// The mosaic of an exposure lit in `lit` (0 red, 1 green, 2 blue): the lit colour's photosites
    /// carry that channel of `negative`, the others a little light that leaked through.
    fn exposure(negative: &ImageF32, lit: usize) -> RawLinear {
        let (w, h) = (negative.width as usize, negative.height as usize);
        let mut data = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let colour = Cfa::RGGB.color_at(x, y);
                let value = if colour == lit {
                    negative.data[(y * w + x) * 3 + lit] * 9_000.0
                } else {
                    120.0
                };
                data.push(value.round().clamp(0.0, 10_000.0) as u16);
            }
        }
        RawLinear {
            width: negative.width,
            height: negative.height,
            cfa: Cfa::RGGB,
            black: [0.0; 4],
            white: 10_000.0,
            data,
        }
    }

    struct Setup {
        dir: PathBuf,
        lib: Option<Library>,
        roll: RollId,
        fake: Fake,
        files: Vec<PathBuf>,
        negatives: Vec<ImageF32>,
    }

    impl Setup {
        /// A library with a roll and `frames` triplets known to the fake decoder.
        fn new(label: &str, frames: u64) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "gtp-import-triplets-{label}-{}-{}",
                std::process::id(),
                FrameId::new()
            ));
            let lib = Library::create(&dir.join("lib"), "Triplets").unwrap();
            let roll = roll_create(
                &lib,
                RollInput {
                    name: "Camera".into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .id;
            let mut fake = Fake(HashMap::new());
            let mut files = Vec::new();
            let mut negatives = Vec::new();
            for frame in 0..frames {
                let negative = synthetic::negative(
                    frame + 1,
                    &Params {
                        width: W,
                        height: H,
                        rebate: [10; 4],
                        pattern: Pattern::Blobs,
                        ..Params::default()
                    },
                )
                .unwrap()
                .image;
                for (lit, name) in ["r", "g", "b"].iter().enumerate() {
                    let path = dir.join(format!("incoming/f{frame}{name}.NEF"));
                    fake.0.insert(path.clone(), Ok(exposure(&negative, lit)));
                    files.push(path);
                }
                negatives.push(negative);
            }
            Self {
                dir,
                lib: Some(lib),
                roll,
                fake,
                files,
                negatives,
            }
        }

        fn lib(&self) -> &Library {
            self.lib.as_ref().unwrap()
        }
    }

    impl Drop for Setup {
        fn drop(&mut self) {
            self.lib = None;
            let _ = std::fs::remove_dir_all(&self.dir);
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

    fn run(s: &Setup, paths: &[PathBuf]) -> Result<ImportResult, AppError> {
        let (ctx, _) = JobCtx::standalone();
        import_triplets(s.lib(), s.roll, paths, &s.fake, &ctx, &Events::default())
    }

    #[test]
    fn three_triplets_become_three_frames_with_three_single_channel_originals_each() {
        let s = Setup::new("three", 3);
        let result = run(&s, &s.files).unwrap();
        assert_eq!(result.frames.len(), 3);
        assert!(result.skipped.is_empty());
        assert!(result.warnings.is_empty());
        assert_eq!(result.frame_ms.len(), 3);

        for (i, id) in result.frames.iter().enumerate() {
            let frame = s.lib().read(|c| c.get_frame(*id)).unwrap();
            assert_eq!(frame.kind, FrameKind::RgbTriplet);
            assert_eq!((frame.width, frame.height, frame.has_ir), (W, H, false));
            assert_eq!(frame.index_in_roll, i as u32 + 1);

            let sources = s.lib().read(|c| c.sources_of(*id)).unwrap();
            // The catalog may list them in any order: each role appears once, with its own file.
            for (role, name) in [
                (SourceRole::R, "r.tif"),
                (SourceRole::G, "g.tif"),
                (SourceRole::B, "b.tif"),
            ] {
                let of_role: Vec<_> = sources.iter().filter(|s| s.role == role).collect();
                assert_eq!(of_role.len(), 1, "{role:?}");
                assert!(
                    of_role[0].rel_path.ends_with(name),
                    "{}",
                    of_role[0].rel_path
                );
            }
            assert_eq!(sources.len(), 3);
            for source in &sources {
                let path = s.lib().resolve(Path::new(&source.rel_path)).unwrap();
                assert!(path.is_file());
                assert!(std::fs::metadata(&path).unwrap().permissions().readonly());
                let plane = read_tiff16(&path, &Limits::default()).unwrap();
                assert_eq!((plane.width, plane.height, plane.channels), (W, H, 1));
                assert_eq!(source.sha256.len(), 64);
            }
            let thumb = s.lib().resolve(&crate::library_fs::thumb_rel(*id)).unwrap();
            assert!(thumb.is_file());
        }
    }

    #[test]
    fn a_stored_frame_loads_as_the_merged_picture_with_each_channel_from_its_exposure() {
        let s = Setup::new("merged", 1);
        let result = run(&s, &s.files).unwrap();
        let id = result.frames[0];
        let sources = s.lib().read(|c| c.sources_of(id)).unwrap();
        let image = read_scan(s.lib(), &sources).unwrap();
        assert_eq!((image.width, image.height, image.channels), (W, H, 3));
        for channel in 0..3 {
            let exposure = s.fake.0[&s.files[channel]].as_ref().unwrap();
            let want = demosaic_channel(exposure, channel).unwrap();
            for (i, w) in want.data.iter().enumerate() {
                let got = image.data[i * 3 + channel];
                assert!(
                    (got - w).abs() < 2.0 / 65_535.0,
                    "channel {channel} sample {i}"
                );
            }
        }
        // The Develop view opens it: the proxy is built from the merged picture.
        let develop = DevelopService::default();
        let info = develop.open(s.lib(), id).unwrap();
        assert_eq!((info.width, info.height), (W, H));
        // The negatives the fake was built from are different frames, so channels differ.
        assert_ne!(s.negatives[0].data[0], 0.0);
    }

    #[test]
    fn the_first_frame_sets_the_rolls_film_base() {
        let s = Setup::new("base", 2);
        assert_eq!(
            s.lib().read(|c| c.get_roll(s.roll)).unwrap().film_base_rgb,
            None
        );
        run(&s, &s.files).unwrap();
        let roll = s.lib().read(|c| c.get_roll(s.roll)).unwrap();
        assert!(roll.film_base_rgb.is_some());
        assert!(roll.film_base_confidence.is_some());
    }

    #[test]
    fn a_bad_triplet_is_skipped_with_all_three_files_and_the_rest_go_on() {
        let mut s = Setup::new("skip", 3);
        s.fake.0.insert(
            s.files[4].clone(),
            Err(AppError::new(
                ErrorCode::UnsupportedFormat,
                "not a camera file",
            )),
        );
        let result = run(&s, &s.files).unwrap();
        assert_eq!(result.frames.len(), 2);
        assert_eq!(result.skipped.len(), 3);
        assert!(result
            .skipped
            .iter()
            .all(|f| f.reason == SkipReason::Unsupported));
        let skipped: Vec<_> = result.skipped.iter().map(|f| f.path.as_str()).collect();
        assert!(skipped.iter().all(|p| p.contains("f1")), "{skipped:?}");
        // No folder or thumbnail is left for the skipped triplet.
        assert_eq!(s.lib().read(|c| c.count_frames(Some(s.roll))).unwrap(), 2);
        let originals = s.lib().root().join("originals").join(s.roll.to_string());
        assert_eq!(std::fs::read_dir(&originals).unwrap().count(), 2);
    }

    #[test]
    fn exposures_of_different_sizes_missing_files_and_oversized_files_are_skipped_by_their_reason()
    {
        let mut s = Setup::new("reasons", 3);
        // Frame 0: a green exposure of another size.
        let mut small = s.fake.0[&s.files[1]].clone().unwrap();
        small.width = 100;
        small.data.truncate(100 * H as usize);
        s.fake.0.insert(s.files[1].clone(), Ok(small));
        // Frame 1: a file that is not there.
        s.fake.0.remove(&s.files[3]);
        // Frame 2: a file above the size limit.
        s.fake.0.insert(
            s.files[6].clone(),
            Err(AppError::new(
                ErrorCode::InvalidInput,
                "This camera file is larger than the app accepts.",
            )),
        );
        let result = run(&s, &s.files).unwrap();
        assert!(result.frames.is_empty());
        let reasons: Vec<SkipReason> = result.skipped.iter().map(|f| f.reason).collect();
        assert_eq!(reasons[..3], [SkipReason::Unsupported; 3]);
        assert_eq!(reasons[3..6], [SkipReason::Unreadable; 3]);
        assert_eq!(reasons[6..], [SkipReason::TooLarge; 3]);
    }

    #[test]
    fn a_cancel_keeps_the_finished_frames_and_leaves_nothing_of_the_unfinished_one() {
        let s = Setup::new("cancel", 4);
        let (ctx, flag) = JobCtx::standalone();
        let events = Events {
            cancel_after: Some((2, flag)),
            ..Events::default()
        };
        let err = import_triplets(s.lib(), s.roll, &s.files, &s.fake, &ctx, &events).unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        assert_eq!(s.lib().read(|c| c.count_frames(Some(s.roll))).unwrap(), 2);
        let originals = s.lib().root().join("originals").join(s.roll.to_string());
        assert_eq!(std::fs::read_dir(&originals).unwrap().count(), 2);
    }

    #[test]
    fn a_count_that_is_not_a_multiple_of_three_is_refused_before_anything_is_read() {
        let s = Setup::new("count", 1);
        for n in [0usize, 1, 2, 4] {
            let paths: Vec<PathBuf> = s.files.iter().cycle().take(n).cloned().collect();
            let err = run(&s, &paths).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{n}");
        }
        assert_eq!(s.lib().read(|c| c.count_frames(Some(s.roll))).unwrap(), 0);
    }

    #[test]
    fn a_roll_that_is_not_there_stops_the_job() {
        let s = Setup::new("noroll", 1);
        let (ctx, _) = JobCtx::standalone();
        let err = import_triplets(
            s.lib(),
            RollId::new(),
            &s.files,
            &s.fake,
            &ctx,
            &Events::default(),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn warnings_of_the_merge_are_reported_for_their_frame() {
        let mut s = Setup::new("warn", 1);
        // Clip the red-lit exposure: every red photosite at the white level.
        let mut red = s.fake.0[&s.files[0]].clone().unwrap();
        for y in 0..H as usize {
            for x in 0..W as usize {
                if Cfa::RGGB.color_at(x, y) == 0 {
                    red.data[y * W as usize + x] = 10_000;
                }
            }
        }
        s.fake.0.insert(s.files[0].clone(), Ok(red));
        let result = run(&s, &s.files).unwrap();
        assert_eq!(result.frames.len(), 1);
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].frame_id, result.frames[0]);
        let ImportWarningKind::Clipped { channel, per_mille } = result.warnings[0].kind else {
            unreachable!("{:?}", result.warnings[0]);
        };
        assert_eq!(channel, gtp_core::TripletChannel::Red);
        assert!(per_mille > 900, "{per_mille}");
    }

    #[test]
    fn the_warning_kinds_map_one_to_one() {
        use gtp_core::TripletChannel;

        let pairs = [
            (
                TripletWarning::ExposureTimeMismatch,
                ImportWarningKind::ExposureTimeMismatch,
            ),
            (TripletWarning::IsoMismatch, ImportWarningKind::IsoMismatch),
            (
                TripletWarning::ApertureMismatch,
                ImportWarningKind::ApertureMismatch,
            ),
            (
                TripletWarning::Clipped {
                    channel: TripletChannel::Blue,
                    fraction: 0.0125,
                },
                ImportWarningKind::Clipped {
                    channel: TripletChannel::Blue,
                    per_mille: 13,
                },
            ),
        ];
        for (warning, kind) in pairs {
            assert_eq!(warning_kind(&warning), kind);
        }
    }

    #[test]
    fn verify_accepts_a_triplet_frame() {
        let s = Setup::new("verify", 1);
        run(&s, &s.files).unwrap();
        let (ctx, _) = JobCtx::standalone();
        let verified = crate::services::verify::verify_library(s.lib(), &ctx).unwrap();
        assert!(verified.problems.is_empty(), "{:?}", verified.problems);
        assert_eq!(verified.files_checked, 3);
    }

    /// Imports the first three real triplets of the private D600 session into a temporary library
    /// and records the time of each frame against the 10 s budget (specification section 11). Run by
    /// hand with `cargo test -p gtp-app --release -- --ignored --nocapture import_triplets`.
    #[test]
    #[ignore = "reads the private D600 samples"]
    fn the_real_d600_roll_imports_within_the_budget() {
        let samples = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../samples/private/Mamiya 645 Kodak Gold 200 120 Trichrome RGB");
        let files: Vec<PathBuf> = (4652..4661)
            .map(|n| samples.join(format!("DSC_{n}.NEF")))
            .collect();
        if !files[0].is_file() {
            eprintln!("no private samples; skipped");
            return;
        }
        let s = Setup::new("real", 0);
        let (ctx, _) = JobCtx::standalone();
        let result = import_triplets(
            s.lib(),
            s.roll,
            &files,
            &gtp_io::RawlerDecoder,
            &ctx,
            &Events::default(),
        )
        .unwrap();
        eprintln!(
            "frame_ms {:?}, warnings {:?}",
            result.frame_ms, result.warnings
        );
        assert_eq!(result.frames.len(), 3);
        assert!(result.skipped.is_empty());
        assert!(
            result.frame_ms.iter().all(|ms| *ms <= 10_000),
            "{:?}",
            result.frame_ms
        );
        for id in &result.frames {
            let sources = s.lib().read(|c| c.sources_of(*id)).unwrap();
            let image = read_scan(s.lib(), &sources).unwrap();
            assert_eq!((image.width, image.height, image.channels), (6016, 4016, 3));
        }
    }
}
