//! The roll-scan job (H-03): scans frames of a roll into the library, one at a time. Every frame the
//! scanner finishes is stored at once, the way an imported TIFF is (originals, hash, starting
//! recipe, thumbnail, catalog row), so a cancel or an error on frame k keeps frames 1 to k-1. The
//! scanner's own frame number is kept with each frame, so a scan can resume at the next one
//! ([`scan_resume_point`]). The first scanned frame of a roll sets its film base, as any first frame
//! does (L-10).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use gtp_core::{AppError, ErrorCode, FrameId, ImportResult, Look, RollId, SkipReason, SkippedFile};
use gtp_ingest::{CancelToken, RawScan, ScanParams, ScannerSource};
use gtp_io::Tiff16;

use crate::jobs::{JobCtx, LibraryEvents};
use crate::services::counters;
use crate::services::import::{clean_up, report_roll_base, store_frame, FrameError, Planes, STEPS};
use crate::services::library::Library;
use crate::services::roll_balance::JobBalance;

/// The stage name reported while scanning.
pub const STAGE: &str = "scanning";

/// The frame number a scan of `roll` should resume at: one after the highest scanner number among
/// its scanned frames, or 1 for a roll with none. Above 40 (the most a roll holds) the roll is done.
pub fn scan_resume_point(library: &Library, roll: RollId) -> Result<u32, AppError> {
    library.read(|c| c.get_roll(roll))?;
    let last = library.read(|c| c.last_scan_number(roll))?;
    Ok(last.map_or(1, |n| n + 1))
}

/// What the UI asks a scan to do: frames `first_frame` to `last_frame` of the roll (counted from 1) on
/// `device_id`. [`ScanRequest::params`] checks it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ScanRequest {
    pub device_id: String,
    pub first_frame: u32,
    pub last_frame: u32,
    pub dpi: u32,
    pub samples: u8,
    pub ir: bool,
    pub autofocus: bool,
}

impl ScanRequest {
    /// The checked scan parameters; `InvalidInput` for anything [`ScanParams::validate`] refuses.
    pub fn params(&self) -> Result<ScanParams, AppError> {
        let params = ScanParams {
            device_id: self.device_id.clone(),
            frames: self.first_frame..=self.last_frame,
            dpi: self.dpi,
            samples: self.samples,
            ir: self.ir,
            autofocus: self.autofocus,
        };
        params.validate()?;
        Ok(params)
    }
}

/// How a frame was scanned, kept with it as JSON.
fn scan_info(scan: &RawScan, ir: bool) -> String {
    serde_json::json!({
        "device": scan.meta.device_id,
        "dpi": scan.dpi,
        "samples": scan.meta.samples,
        "autofocus": scan.meta.autofocus,
        "ir": ir,
    })
    .to_string()
}

/// Scans `params.frames` of `roll` with `scanner`, storing each frame as it completes. `look` is the
/// Starting look setting. The result lists the frames stored, like an import's. A cancel ends with
/// `Cancelled` and a scanner or storing error with that error; frames already stored stay.
pub fn scan_roll(
    library: &Library,
    roll: RollId,
    scanner: &dyn ScannerSource,
    params: &ScanParams,
    ctx: &JobCtx,
    events: &dyn LibraryEvents,
    look: Look,
) -> Result<ImportResult, AppError> {
    params.validate()?;
    let before = library.read(|c| c.get_roll(roll))?;
    let total = params.frame_count().max(1) as f32;
    let first = *params.frames.start();
    let mut result = ImportResult::default();
    let mut balance = JobBalance::default();

    // The scanner polls its own token: forward the job's cancel to it while the scan runs.
    let token = CancelToken::default();
    let finished = AtomicBool::new(false);
    let scanned = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !finished.load(Ordering::SeqCst) {
                if ctx.is_cancelled() {
                    token.cancel();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        });
        let outcome = scanner.scan(
            params,
            &mut |number, scan| {
                ctx.check_cancelled()?;
                let started = Instant::now();
                let done = number.saturating_sub(first) as f32;
                let progress = |step: f32| ctx.progress(STAGE, (done + step / STEPS) / total);
                progress(0.0);
                let frame_id = FrameId::new();
                match store_scan(library, roll, frame_id, number, &scan, ctx, look, &progress) {
                    Ok(estimate) => {
                        result.frames.push(frame_id);
                        balance.record(frame_id, estimate);
                        counters::count_import(library);
                        result
                            .frame_ms
                            .push(u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX));
                        events.library_changed(Some(roll));
                        Ok(())
                    }
                    Err(err) => {
                        clean_up(library, roll, frame_id);
                        Err(err)
                    }
                }
            },
            // Progress is reported per stored frame, with its steps, not by the scanner.
            &|_| {},
            &token,
        );
        finished.store(true, Ordering::SeqCst);
        outcome
    });
    // The roll's colour balance is settled after a cancelled or failed scan too, so the frames it
    // stored agree with the roll.
    let settled = balance.finish(library, roll);
    if let Err(err) = scanned {
        if err.code != ErrorCode::Cancelled {
            counters::count_scan_failed(library);
        }
        if let Ok(settled) = settled {
            settled.drop_thumbnails(library);
            if !settled.rebalanced.is_empty() {
                events.library_changed(Some(roll));
            }
        }
        return Err(err);
    }
    settled?.report(&mut result);
    ctx.progress(STAGE, 1.0);
    report_roll_base(library, &before, &mut result)?;
    Ok(result)
}

/// Stores one scanned frame; a frame that could not be stored stops the scan (a scan has no file
/// to skip), with the reason in the error.
#[allow(clippy::too_many_arguments)]
fn store_scan(
    library: &Library,
    roll: RollId,
    frame_id: FrameId,
    number: u32,
    scan: &RawScan,
    ctx: &JobCtx,
    look: Look,
    progress: &dyn Fn(f32),
) -> Result<Option<[f32; 3]>, AppError> {
    let rgb = Tiff16 {
        width: scan.width,
        height: scan.height,
        channels: 3,
        data: scan.rgb16.clone(),
    };
    let ir = scan.ir16.as_ref().map(|plane| Tiff16 {
        width: scan.width,
        height: scan.height,
        channels: 1,
        data: plane.clone(),
    });
    let name = format!("scanner frame {number}");
    let info = scan_info(scan, ir.is_some());
    let stored = store_frame(
        library,
        roll,
        frame_id,
        &Planes {
            rgb: &rgb,
            ir: ir.as_ref(),
            picked_scan: Path::new(&name),
            picked_ir: None,
        },
        ctx,
        look,
        progress,
        Some((number, &info)),
    );
    match stored {
        Ok((_, estimate)) => Ok(estimate),
        Err(FrameError::Stop(err)) => Err(err),
        Err(FrameError::Skip(SkippedFile {
            reason, details, ..
        })) => Err(AppError::new(
            ErrorCode::ScanFailed,
            format!(
                "Frame {number} could not be stored ({}).",
                skip_text(reason)
            ),
        )
        .with_details(details.unwrap_or_default())),
    }
}

fn skip_text(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::VerifyFailed => "it read back different",
        SkipReason::Unreadable => "it could not be read back",
        _ => "it was refused",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use gtp_core::testing::synthetic::{self, Params, Pattern};
    use gtp_ingest::{FixtureSource, FIXTURE_DEVICE_ID};
    use gtp_io::{from_f32_linear, merge_rgbi, write_tiff16, Compression};

    use super::*;
    use crate::services::roll_frame::{roll_create, RollInput};
    use crate::services::testkit::TempDir;

    #[derive(Default)]
    struct Events {
        changed: Mutex<u32>,
    }

    impl LibraryEvents for Events {
        fn library_changed(&self, _: Option<RollId>) {
            *self.changed.lock().unwrap() += 1;
        }
    }

    struct Setup {
        library: Library,
        roll: RollId,
        source: FixtureSource,
        /// Last, so the folder goes after the library has closed its database.
        _temp: TempDir,
    }

    impl Setup {
        /// A library with one roll, and a fixture folder of `frames` synthetic negatives with
        /// infrared (as 4-channel files).
        fn new(label: &str, frames: u32) -> Self {
            let temp = TempDir::new(&format!("scan-{label}"));
            let dir = temp.path().to_owned();
            let library = Library::create(&dir.join("lib"), "Scans").unwrap();
            let roll = roll_create(
                &library,
                RollInput {
                    name: "Roll".into(),
                    ..RollInput::default()
                },
            )
            .unwrap()
            .id;
            let fixtures = dir.join("fixtures");
            std::fs::create_dir_all(&fixtures).unwrap();
            for n in 1..=frames {
                let s = synthetic::negative(
                    u64::from(n),
                    &Params {
                        width: 200,
                        height: 140,
                        rebate: [10; 4],
                        pattern: Pattern::Blobs,
                        with_ir: true,
                        dust_specks: 2,
                        ..Params::default()
                    },
                )
                .unwrap();
                let tiff = merge_rgbi(
                    &from_f32_linear(&s.image),
                    &from_f32_linear(s.ir.as_ref().unwrap()),
                )
                .unwrap();
                write_tiff16(
                    &fixtures.join(format!("{n:04}.tif")),
                    &tiff,
                    Compression::None,
                )
                .unwrap();
            }
            Self {
                source: FixtureSource::new(&fixtures),
                library,
                roll,
                _temp: temp,
            }
        }

        fn scan(
            &self,
            frames: std::ops::RangeInclusive<u32>,
            ctx: &JobCtx,
        ) -> Result<ImportResult, AppError> {
            scan_roll(
                &self.library,
                self.roll,
                &self.source,
                &ScanParams::new(FIXTURE_DEVICE_ID, frames),
                ctx,
                &Events::default(),
                Look::Neutral,
            )
        }

        fn frames(&self) -> Vec<gtp_core::Frame> {
            self.library.read(|c| c.list_frames(self.roll)).unwrap()
        }
    }

    #[test]
    fn every_frame_is_stored_with_originals_hash_thumbnail_and_row() {
        let s = Setup::new("all", 3);
        let (ctx, _) = JobCtx::standalone();
        let events = Events::default();
        let result = scan_roll(
            &s.library,
            s.roll,
            &s.source,
            &ScanParams::new(FIXTURE_DEVICE_ID, 1..=3),
            &ctx,
            &events,
            Look::Neutral,
        )
        .unwrap();
        assert_eq!(result.frames.len(), 3);
        assert_eq!(result.frame_ms.len(), 3);
        assert_eq!(*events.changed.lock().unwrap(), 3);
        for id in &result.frames {
            let frame = s.library.read(|c| c.get_frame(*id)).unwrap();
            assert_eq!((frame.width, frame.height), (200, 140));
            assert!(frame.has_ir);
            let sources = s.library.read(|c| c.sources_of(*id)).unwrap();
            assert_eq!(sources.len(), 2, "scan and infrared");
            for source in sources {
                assert_eq!(source.sha256.len(), 64);
                assert!(s
                    .library
                    .resolve(Path::new(&source.rel_path))
                    .unwrap()
                    .is_file());
            }
            assert!(s.library.read(|c| c.load_recipe(*id)).unwrap().is_some());
            assert!(crate::services::roll_frame::frame_thumbnail(&s.library, *id).is_ok());
        }
        let counted = counters::read(&s.library).unwrap();
        assert_eq!(counted.frames_imported, 3);
        assert_eq!(counters::read(&s.library).unwrap().scans_failed, 0);
    }

    #[test]
    fn the_roll_film_base_comes_from_the_first_scanned_frame() {
        let s = Setup::new("base", 2);
        assert!(s
            .library
            .read(|c| c.get_roll(s.roll))
            .unwrap()
            .film_base_rgb
            .is_none());
        let (ctx, _) = JobCtx::standalone();
        s.scan(1..=1, &ctx).unwrap();
        let roll = s.library.read(|c| c.get_roll(s.roll)).unwrap();
        assert!(roll.film_base_rgb.is_some());
    }

    #[test]
    fn a_scanner_job_gives_the_roll_one_balance_and_a_cancelled_one_leaves_it_consistent() {
        let axes = |s: &Setup, id: FrameId| {
            let r = s.library.read(|c| c.load_recipe(id)).unwrap().unwrap();
            [r.color.red_cyan, r.color.green_magenta, r.color.blue_yellow]
        };
        let s = Setup::new("balance", 3);
        let (ctx, _) = JobCtx::standalone();
        let result = s.scan(1..=3, &ctx).unwrap();
        let balance = s
            .library
            .read(|c| c.get_roll(s.roll))
            .unwrap()
            .color_balance
            .expect("the scanned frames were judged");
        for id in &result.frames {
            assert_eq!(axes(&s, *id), balance);
        }
        assert!(result.warnings.is_empty());

        // Cancelled after the second of four frames: the two stored frames and the roll agree.
        let s = Setup::new("balance-cancel", 4);
        let (ctx, flag) = JobCtx::standalone();
        struct CancelOnSecond(crate::jobs::CancelFlag, Mutex<u32>);
        impl LibraryEvents for CancelOnSecond {
            fn library_changed(&self, _: Option<RollId>) {
                let mut n = self.1.lock().unwrap();
                *n += 1;
                if *n == 2 {
                    self.0.cancel();
                }
            }
        }
        let err = scan_roll(
            &s.library,
            s.roll,
            &s.source,
            &ScanParams::new(FIXTURE_DEVICE_ID, 1..=4),
            &ctx,
            &CancelOnSecond(flag, Mutex::new(0)),
            Look::Neutral,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        let balance = s
            .library
            .read(|c| c.get_roll(s.roll))
            .unwrap()
            .color_balance
            .expect("the two frames were judged");
        let frames = s.frames();
        assert_eq!(frames.len(), 2);
        for f in &frames {
            assert_eq!(axes(&s, f.id), balance);
            assert!(f.edited_at.is_none());
        }

        // Frames scanned later follow the roll's balance at once.
        let (ctx, _) = JobCtx::standalone();
        let rest = s.scan(3..=4, &ctx).unwrap();
        assert!(rest.rebalanced.is_empty());
        for id in &rest.frames {
            assert_eq!(axes(&s, *id), balance);
        }
    }

    #[test]
    fn a_cancel_after_frame_k_keeps_the_frames_up_to_k_and_resume_continues() {
        let s = Setup::new("cancel", 4);
        let (ctx, flag) = JobCtx::standalone();
        // Cancel as the second frame is announced to the library.
        struct CancelOnSecond(crate::jobs::CancelFlag, Mutex<u32>);
        impl LibraryEvents for CancelOnSecond {
            fn library_changed(&self, _: Option<RollId>) {
                let mut n = self.1.lock().unwrap();
                *n += 1;
                if *n == 2 {
                    self.0.cancel();
                }
            }
        }
        let events = CancelOnSecond(flag, Mutex::new(0));
        let err = scan_roll(
            &s.library,
            s.roll,
            &s.source,
            &ScanParams::new(FIXTURE_DEVICE_ID, 1..=4),
            &ctx,
            &events,
            Look::Neutral,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        assert_eq!(s.frames().len(), 2, "frames 1 and 2 stay");
        assert_eq!(scan_resume_point(&s.library, s.roll).unwrap(), 3);
        // Nothing of a half-stored frame is left on disk.
        let originals = s.library.resolve(Path::new("originals")).unwrap();
        let on_disk = std::fs::read_dir(originals.join(s.roll.to_string()))
            .map(|d| d.count())
            .unwrap_or(0);
        assert_eq!(on_disk, 2);
        assert_eq!(
            counters::read(&s.library).unwrap().scans_failed,
            0,
            "a cancel is not a failure"
        );

        // Resume at frame 3 with a fresh job: frames 3 and 4 arrive under their own numbers.
        let (ctx, _) = JobCtx::standalone();
        let rest = s.scan(3..=4, &ctx).unwrap();
        assert_eq!(rest.frames.len(), 2);
        assert_eq!(s.frames().len(), 4);
        assert_eq!(scan_resume_point(&s.library, s.roll).unwrap(), 5);
    }

    #[test]
    fn a_scanner_error_on_frame_k_keeps_the_frames_before_it_and_is_counted() {
        // Three scans in the folder, four asked for: frame 4 is missing.
        let s = Setup::new("error", 3);
        let (ctx, _) = JobCtx::standalone();
        let err = s.scan(1..=4, &ctx).unwrap_err();
        assert_eq!(err.code, ErrorCode::ScanFailed);
        assert_eq!(s.frames().len(), 3);
        assert_eq!(scan_resume_point(&s.library, s.roll).unwrap(), 4);
        assert_eq!(counters::read(&s.library).unwrap().scans_failed, 1);
    }

    #[test]
    fn a_storing_error_stops_the_scan_and_leaves_no_files_for_that_frame() {
        let s = Setup::new("store", 3);
        let (ctx, _) = JobCtx::standalone();
        // A read-only library cannot store: the first frame fails to be recorded.
        let originals = s.library.resolve(Path::new("originals")).unwrap();
        std::fs::create_dir_all(&originals).unwrap();
        // A file where the roll's folder should be makes writing the frame's originals fail.
        std::fs::write(originals.join(s.roll.to_string()), b"in the way").unwrap();
        let err = s.scan(1..=3, &ctx).unwrap_err();
        assert!(
            matches!(err.code, ErrorCode::Io | ErrorCode::ScanFailed),
            "{err:?}"
        );
        assert!(s.frames().is_empty());
        assert_eq!(scan_resume_point(&s.library, s.roll).unwrap(), 1);
    }

    #[test]
    fn the_resume_point_starts_at_1_needs_the_roll_and_follows_the_last_scanned_number() {
        let s = Setup::new("resume", 2);
        assert_eq!(scan_resume_point(&s.library, s.roll).unwrap(), 1);
        let err = scan_resume_point(&s.library, RollId::new()).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        let (ctx, _) = JobCtx::standalone();
        s.scan(2..=2, &ctx).unwrap();
        assert_eq!(
            scan_resume_point(&s.library, s.roll).unwrap(),
            3,
            "frame 2 was scanned alone: the next is 3"
        );
    }

    #[test]
    fn a_request_becomes_checked_parameters() {
        let ok = ScanRequest {
            device_id: "usb:1".into(),
            first_frame: 2,
            last_frame: 5,
            dpi: 2000,
            samples: 4,
            ir: false,
            autofocus: true,
        };
        let p = ok.params().unwrap();
        assert_eq!(
            (p.frames, p.dpi, p.samples, p.ir, p.autofocus),
            (2..=5, 2000, 4, false, true)
        );
        for bad in [
            ScanRequest {
                first_frame: 0,
                ..ok.clone()
            },
            ScanRequest {
                last_frame: 41,
                ..ok.clone()
            },
            ScanRequest {
                first_frame: 6,
                ..ok.clone()
            },
            ScanRequest {
                dpi: 300,
                ..ok.clone()
            },
            ScanRequest {
                samples: 3,
                ..ok.clone()
            },
            ScanRequest {
                device_id: String::new(),
                ..ok.clone()
            },
        ] {
            assert_eq!(
                bad.params().unwrap_err().code,
                ErrorCode::InvalidInput,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn bad_parameters_and_a_missing_roll_are_refused_before_any_scan() {
        let s = Setup::new("bad", 1);
        let (ctx, _) = JobCtx::standalone();
        let bad = ScanParams::new("", 1..=1);
        let err = scan_roll(
            &s.library,
            s.roll,
            &s.source,
            &bad,
            &ctx,
            &Events::default(),
            Look::Neutral,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let err = scan_roll(
            &s.library,
            RollId::new(),
            &s.source,
            &ScanParams::new(FIXTURE_DEVICE_ID, 1..=1),
            &ctx,
            &Events::default(),
            Look::Neutral,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }
}
