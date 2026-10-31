//! Library verify (L-07, docs/specification.md section 5.3): re-checks every original against what
//! was recorded at import, so a damaged, replaced or deleted file is found before it is needed.
//!
//! It only reads and reports. For each recorded original it looks, in this order, for: a missing
//! file, an unreadable one (or a recorded path leading outside the library), a different size
//! (reported without hashing), and a different SHA-256. It also counts frames without a thumbnail
//! (a rebuildable cache, not an error) and lists non-empty folders under `originals/` that belong to
//! no roll or frame of the catalog. Progress counts bytes, and a cancel is honoured in the middle of
//! a large file. A library made by a newer app version can be verified too.

use std::collections::HashSet;
use std::path::Path;

use gtp_core::{AppError, ErrorCode, FrameId, RollId, VerifyProblem, VerifyReason, VerifyResult};
use gtp_io::sha256_file_with;

use crate::jobs::JobCtx;
use crate::library_fs::{thumb_rel, ORIGINALS_DIR};
use crate::paths::display_path;
use crate::services::library::{Library, VERIFY_AT_KEY, VERIFY_PROBLEMS_KEY};

/// The stage name reported while verifying.
pub const STAGE: &str = "verifying";

fn problem(
    original: &gtp_catalog::RecordedOriginal,
    reason: VerifyReason,
    details: Option<String>,
) -> VerifyProblem {
    VerifyProblem {
        frame_id: original.source.frame_id,
        roll_id: original.roll_id,
        role: original.source.role,
        rel_path: original.source.rel_path.clone(),
        reason,
        details,
    }
}

fn cancelled() -> AppError {
    AppError::new(ErrorCode::Cancelled, "The job was cancelled.")
}

/// Checks one original. `Ok(None)` when it checks out; `Err(Cancelled)` when stopped part-way.
fn check_original(
    library: &Library,
    original: &gtp_catalog::RecordedOriginal,
    ctx: &JobCtx,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<Option<VerifyProblem>, AppError> {
    let source = &original.source;
    let Ok(path) = library.resolve(Path::new(&source.rel_path)) else {
        return Ok(Some(problem(
            original,
            VerifyReason::Unreadable,
            Some("the recorded path is outside the library".into()),
        )));
    };
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Some(problem(original, VerifyReason::Missing, None)))
        }
        Err(err) => {
            return Ok(Some(problem(
                original,
                VerifyReason::Unreadable,
                Some(err.to_string()),
            )))
        }
    };
    if meta.len() != source.bytes {
        return Ok(Some(problem(
            original,
            VerifyReason::WrongSize,
            Some(format!(
                "found {} bytes, recorded {}",
                meta.len(),
                source.bytes
            )),
        )));
    }
    let hashed = sha256_file_with(&path, |read| {
        on_bytes(read);
        !ctx.is_cancelled()
    });
    match hashed {
        Ok(None) => Err(cancelled()),
        Ok(Some(hash)) if hash == source.sha256 => Ok(None),
        Ok(Some(_)) => Ok(Some(problem(original, VerifyReason::Changed, None))),
        Err(err) if err.code == ErrorCode::NotFound => {
            Ok(Some(problem(original, VerifyReason::Missing, None)))
        }
        Err(err) => Ok(Some(problem(
            original,
            VerifyReason::Unreadable,
            err.details.or(Some(err.message)),
        ))),
    }
}

/// Non-empty folders under `originals/` that the catalog does not know, relative to the library.
fn stray_folders(
    library: &Library,
    rolls: &HashSet<RollId>,
    frames: &HashSet<(RollId, FrameId)>,
) -> Vec<String> {
    let Ok(originals) = library.resolve(Path::new(ORIGINALS_DIR)) else {
        return Vec::new();
    };
    let list = |dir: &Path| -> Vec<std::path::PathBuf> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        entries.sort();
        entries
    };
    let not_empty = |dir: &Path| std::fs::read_dir(dir).is_ok_and(|mut rd| rd.next().is_some());
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
    let rel = |p: &Path| {
        p.strip_prefix(library.root()).map_or_else(
            |_| display_path(p),
            |r| r.to_string_lossy().replace('\\', "/"),
        )
    };

    let mut stray = Vec::new();
    for roll_dir in list(&originals) {
        let roll = name(&roll_dir).and_then(|n| n.parse::<RollId>().ok());
        let Some(roll) = roll.filter(|r| rolls.contains(r)) else {
            if not_empty(&roll_dir) || roll_dir.is_file() {
                stray.push(rel(&roll_dir));
            }
            continue;
        };
        for frame_dir in list(&roll_dir) {
            let known = name(&frame_dir)
                .and_then(|n| n.parse::<FrameId>().ok())
                .is_some_and(|f| frames.contains(&(roll, f)));
            if !known && (not_empty(&frame_dir) || frame_dir.is_file()) {
                stray.push(rel(&frame_dir));
            }
        }
    }
    stray
}

/// Verifies the whole library as described in the module docs. Ends with `Cancelled` when
/// cancelled. Records the time and problem count in the library afterwards, unless it is read-only.
pub fn verify_library(library: &Library, ctx: &JobCtx) -> Result<VerifyResult, AppError> {
    let originals = library.read(|c| c.recorded_originals())?;
    let (frame_keys, rolls) = library.read(|c| {
        let rolls: HashSet<RollId> = c.list_rolls()?.into_iter().map(|r| r.id).collect();
        Ok((c.frame_keys()?, rolls))
    })?;
    let total_bytes: u64 = originals.iter().map(|o| o.source.bytes).sum();
    let mut result = VerifyResult::default();

    let mut done_bytes = 0u64;
    ctx.progress(STAGE, 0.0);
    for (i, original) in originals.iter().enumerate() {
        ctx.check_cancelled()?;
        let before = done_bytes;
        let fraction = |bytes: u64| {
            if total_bytes == 0 {
                i as f32 / originals.len().max(1) as f32
            } else {
                bytes as f32 / total_bytes as f32
            }
        };
        let mut on_bytes = |read: u64| ctx.progress(STAGE, fraction(before + read));
        let found = check_original(library, original, ctx, &mut on_bytes)?;
        done_bytes += original.source.bytes;
        ctx.progress(STAGE, fraction(done_bytes));
        result.files_checked += 1;
        result.bytes_checked += original.source.bytes;
        result.problems.extend(found);
    }

    let frames: HashSet<(RollId, FrameId)> = frame_keys.iter().copied().collect();
    result.thumbnails_missing = u32::try_from(
        frame_keys
            .iter()
            .filter(|(_, frame)| {
                !library
                    .resolve(&thumb_rel(*frame))
                    .is_ok_and(|p| p.is_file())
            })
            .count(),
    )
    .unwrap_or(u32::MAX);
    result.stray_folders = stray_folders(library, &rolls, &frames);
    ctx.progress(STAGE, 1.0);

    // Remember when, and with what outcome. A read-only library cannot be written to, which is fine.
    let problems = u32::try_from(result.problems.len()).unwrap_or(u32::MAX);
    match library.write(|c| {
        let now = c.now()?;
        c.meta_set(VERIFY_AT_KEY, &now)?;
        c.meta_set(VERIFY_PROBLEMS_KEY, &problems.to_string())
    }) {
        Err(err) if err.code != ErrorCode::LibraryTooNew => return Err(err),
        _ => {}
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use gtp_catalog::SCHEMA_VERSION;
    use gtp_core::{Frame, FrameKind, JobId, JobResult, Recipe, SourceFile, SourceRole};
    use gtp_io::{mark_read_only, sha256_file};

    use super::*;
    use crate::jobs::{CancelFlag, JobSink};
    use crate::library_fs::{originals_rel, DB_FILE};
    use crate::services::roll_frame::{remove_tree, roll_create, RollInput};

    /// Runs on each progress report (a test uses it to cancel from inside the job).
    type ProgressHook = Box<dyn Fn(f32) + Send>;

    /// Records every progress fraction, and can run a hook on each.
    #[derive(Default)]
    struct Recorder {
        fractions: Mutex<Vec<f32>>,
        hook: Mutex<Option<ProgressHook>>,
    }

    impl JobSink for Recorder {
        fn progress(&self, _: JobId, _: &str, fraction: f32) {
            self.fractions.lock().unwrap().push(fraction);
            if let Some(hook) = self.hook.lock().unwrap().as_ref() {
                hook(fraction);
            }
        }
        fn done(&self, _: JobId, _: JobResult) {}
        fn error(&self, _: JobId, _: AppError) {}
    }

    struct TempLib {
        dir: PathBuf,
        lib: Option<Library>,
        roll: RollId,
    }

    struct Added {
        frame: FrameId,
        scan: PathBuf,
        ir: PathBuf,
    }

    impl TempLib {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "gtp-verify-{label}-{}-{}",
                std::process::id(),
                FrameId::new()
            ));
            let lib = Library::create(&dir, "Test").unwrap();
            let roll = roll_create(
                &lib,
                RollInput {
                    name: "R".into(),
                    ..RollInput::default()
                },
            )
            .unwrap()
            .id;
            Self {
                dir,
                lib: Some(lib),
                roll,
            }
        }

        fn lib(&self) -> &Library {
            self.lib.as_ref().unwrap()
        }

        /// Adds a frame with a read-only scan of `size` bytes, an infrared file and a thumbnail,
        /// recorded in the catalog exactly as an import would.
        fn add(&self, size: usize) -> Added {
            let id = FrameId::new();
            let dir = self.lib().resolve(&originals_rel(self.roll, id)).unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            let mut sources = Vec::new();
            let mut paths = Vec::new();
            for (role, name, len) in [
                (SourceRole::Rgb, "scan.tif", size),
                (SourceRole::Ir, "ir.tif", size / 3 + 1),
            ] {
                let path = dir.join(name);
                let data: Vec<u8> = (0..len).map(|i| (i * 31 + 7) as u8).collect();
                std::fs::write(&path, &data).unwrap();
                mark_read_only(&path).unwrap();
                sources.push(SourceFile {
                    frame_id: id,
                    role,
                    rel_path: format!("originals/{}/{id}/{name}", self.roll),
                    sha256: sha256_file(&path).unwrap(),
                    bytes: data.len() as u64,
                });
                paths.push(path);
            }
            let frame = Frame {
                id,
                roll_id: self.roll,
                index_in_roll: 0,
                kind: FrameKind::ScanRgbIr,
                width: 10,
                height: 10,
                has_ir: true,
                note: String::new(),
                created_at: String::new(),
                edited_at: None,
            };
            self.lib()
                .write(|c| c.insert_imported_frame(&frame, &sources, &Recipe::default()))
                .unwrap();
            std::fs::write(self.lib().resolve(&thumb_rel(id)).unwrap(), b"jpg").unwrap();
            let ir = paths.pop().unwrap();
            let scan = paths.pop().unwrap();
            Added {
                frame: id,
                scan,
                ir,
            }
        }

        fn verify(&self) -> Result<VerifyResult, AppError> {
            let (ctx, _) = JobCtx::standalone();
            verify_library(self.lib(), &ctx)
        }
    }

    impl Drop for TempLib {
        fn drop(&mut self) {
            self.lib = None;
            let mut left = Vec::new();
            remove_tree(&self.dir, &mut left);
        }
    }

    /// Lets a test change a read-only original, as damage or tampering would.
    #[allow(clippy::permissions_set_readonly_false)]
    fn unlock(path: &Path) {
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    fn problem_of(result: &VerifyResult, frame: FrameId, role: SourceRole) -> &VerifyProblem {
        result
            .problems
            .iter()
            .find(|p| p.frame_id == frame && p.role == role)
            .unwrap_or_else(|| {
                unreachable!("no problem for {frame} {role:?}: {:?}", result.problems)
            })
    }

    #[test]
    fn a_clean_library_reports_nothing_and_remembers_when_it_was_checked() {
        let t = TempLib::new("clean");
        let added: Vec<Added> = (0..3).map(|i| t.add(50_000 + i * 1000)).collect();
        assert!(t.lib().details().unwrap().last_verify_at.is_none());
        let result = t.verify().unwrap();
        assert_eq!(result.files_checked, 6);
        let expected: u64 = added
            .iter()
            .map(|a| {
                std::fs::metadata(&a.scan).unwrap().len() + std::fs::metadata(&a.ir).unwrap().len()
            })
            .sum();
        assert_eq!(result.bytes_checked, expected);
        assert!(result.problems.is_empty(), "{:?}", result.problems);
        assert_eq!(result.thumbnails_missing, 0);
        assert!(
            result.stray_folders.is_empty(),
            "{:?}",
            result.stray_folders
        );
        let details = t.lib().details().unwrap();
        assert!(details.last_verify_at.is_some_and(|d| d.ends_with('Z')));
        assert_eq!(details.last_verify_problems, Some(0));
    }

    #[test]
    fn a_tampered_a_deleted_and_a_truncated_original_are_found() {
        let t = TempLib::new("damage");
        let (tampered, deleted, truncated, grown, fine) = (
            t.add(40_000),
            t.add(40_000),
            t.add(40_000),
            t.add(40_000),
            t.add(40_000),
        );

        // One byte changed in the middle: same size, different content.
        unlock(&tampered.scan);
        let mut data = std::fs::read(&tampered.scan).unwrap();
        data[20_000] ^= 0x01;
        std::fs::write(&tampered.scan, data).unwrap();
        mark_read_only(&tampered.scan).unwrap();
        unlock(&deleted.ir);
        std::fs::remove_file(&deleted.ir).unwrap();
        unlock(&truncated.scan);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&truncated.scan)
            .unwrap()
            .set_len(1234)
            .unwrap();
        unlock(&grown.scan);
        std::fs::write(&grown.scan, vec![0u8; 50_000]).unwrap();

        let result = t.verify().unwrap();
        assert_eq!(result.problems.len(), 4, "{:?}", result.problems);
        let changed = problem_of(&result, tampered.frame, SourceRole::Rgb);
        assert_eq!(changed.reason, VerifyReason::Changed);
        assert_eq!(changed.roll_id, t.roll);
        assert!(changed.rel_path.ends_with("scan.tif"));
        assert_eq!(
            problem_of(&result, deleted.frame, SourceRole::Ir).reason,
            VerifyReason::Missing
        );
        let short = problem_of(&result, truncated.frame, SourceRole::Rgb);
        assert_eq!(short.reason, VerifyReason::WrongSize);
        assert!(short.details.as_deref().is_some_and(|d| d.contains("1234")));
        assert_eq!(
            problem_of(&result, grown.frame, SourceRole::Rgb).reason,
            VerifyReason::WrongSize
        );
        assert!(!result.problems.iter().any(|p| p.frame_id == fine.frame));
        assert_eq!(result.files_checked, 10, "every file is examined");
        assert_eq!(t.lib().details().unwrap().last_verify_problems, Some(4));
    }

    /// Keeps `file` from being read until the returned value is dropped.
    #[cfg(windows)]
    fn lock(file: &Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        // Opened without any sharing: nobody else may open it while this handle lives.
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(file)
            .unwrap()
    }

    #[cfg(unix)]
    fn lock(file: &Path) -> impl Drop {
        use std::os::unix::fs::PermissionsExt;
        struct Restore(PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o644));
            }
        }
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o000)).unwrap();
        Restore(file.to_path_buf())
    }

    #[test]
    fn a_file_that_cannot_be_read_and_a_path_outside_the_library_are_unreadable() {
        let t = TempLib::new("unreadable");
        let a = t.add(30_000);
        let b = t.add(30_000);
        let held = lock(&a.scan);
        // A catalog row whose path leads out of the library is never followed.
        t.lib()
            .write(|c| {
                c.insert_source(&SourceFile {
                    frame_id: b.frame,
                    role: SourceRole::R,
                    rel_path: "../outside.tif".into(),
                    sha256: "00".repeat(32),
                    bytes: 5,
                })
            })
            .unwrap();
        let result = t.verify().unwrap();
        drop(held);
        let locked = problem_of(&result, a.frame, SourceRole::Rgb);
        assert_eq!(locked.reason, VerifyReason::Unreadable);
        assert!(locked.details.is_some());
        let outside = problem_of(&result, b.frame, SourceRole::R);
        assert_eq!(outside.reason, VerifyReason::Unreadable);
        assert!(outside
            .details
            .as_deref()
            .is_some_and(|d| d.contains("outside")));
        assert_eq!(result.problems.len(), 2, "{:?}", result.problems);
    }

    #[test]
    fn missing_thumbnails_and_stray_folders_are_reported_but_nothing_is_touched() {
        let t = TempLib::new("extras");
        let a = t.add(10_000);
        let _b = t.add(10_000);
        std::fs::remove_file(t.lib().resolve(&thumb_rel(a.frame)).unwrap()).unwrap();

        let originals = t.lib().root().join("originals");
        let make = |rel: PathBuf, file: &str| {
            let dir = originals.join(rel);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(file), b"x").unwrap();
        };
        let unknown_roll = RollId::new();
        let unknown_frame = FrameId::new();
        make(PathBuf::from(unknown_roll.to_string()), "left-over.tif");
        make(PathBuf::from("not-a-roll"), "x.tif");
        make(
            PathBuf::from(t.roll.to_string()).join(unknown_frame.to_string()),
            "scan.tif",
        );
        make(PathBuf::from(t.roll.to_string()).join("junk"), "x");
        // Empty folders are not worth reporting.
        std::fs::create_dir_all(
            originals
                .join(t.roll.to_string())
                .join(FrameId::new().to_string()),
        )
        .unwrap();
        std::fs::create_dir_all(originals.join(RollId::new().to_string())).unwrap();
        // A file loose in `originals/` is reported too.
        std::fs::write(originals.join("loose.txt"), b"x").unwrap();

        let result = t.verify().unwrap();
        assert!(result.problems.is_empty());
        assert_eq!(result.thumbnails_missing, 1);
        let mut expected = vec![
            format!("originals/{unknown_roll}"),
            "originals/not-a-roll".to_owned(),
            format!("originals/{}/{unknown_frame}", t.roll),
            format!("originals/{}/junk", t.roll),
            "originals/loose.txt".to_owned(),
        ];
        expected.sort();
        let mut found = result.stray_folders.clone();
        found.sort();
        assert_eq!(found, expected);
        // Reported, never deleted.
        assert!(originals.join("not-a-roll").join("x.tif").exists());
        assert!(a.scan.exists());
    }

    #[test]
    fn progress_only_moves_forward_and_ends_at_one() {
        let t = TempLib::new("progress");
        for _ in 0..3 {
            t.add(2 * 1024 * 1024 + 5);
        }
        let recorder = Arc::new(Recorder::default());
        let (ctx, _) = JobCtx::standalone_with(recorder.clone());
        let result = verify_library(t.lib(), &ctx).unwrap();
        assert!(result.problems.is_empty());
        let fractions = recorder.fractions.lock().unwrap().clone();
        assert_eq!(fractions.first(), Some(&0.0));
        assert_eq!(fractions.last(), Some(&1.0));
        assert!(
            fractions.len() > 6,
            "reports within files, not only between: {}",
            fractions.len()
        );
        assert!(fractions.windows(2).all(|w| w[0] <= w[1]), "{fractions:?}");
    }

    #[test]
    fn cancelling_before_the_start_and_inside_a_large_file_stops_the_job() {
        let t = TempLib::new("cancel");
        t.add(4 * 1024 * 1024);

        let (ctx, flag) = JobCtx::standalone();
        flag.cancel();
        assert_eq!(
            verify_library(t.lib(), &ctx).unwrap_err().code,
            ErrorCode::Cancelled
        );

        // Cancel from inside the first progress report that lies within the first file.
        let recorder = Arc::new(Recorder::default());
        let (ctx, flag) = JobCtx::standalone_with(recorder.clone());
        let flag: Arc<Mutex<CancelFlag>> = Arc::new(Mutex::new(flag));
        let hook_flag = Arc::clone(&flag);
        *recorder.hook.lock().unwrap() = Some(Box::new(move |fraction| {
            if fraction > 0.0 && fraction < 1.0 {
                hook_flag.lock().unwrap().cancel();
            }
        }));
        let err = verify_library(t.lib(), &ctx).unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        let fractions = recorder.fractions.lock().unwrap().clone();
        assert!(
            fractions.last().is_some_and(|f| *f < 1.0),
            "stopped before the end: {fractions:?}"
        );
        assert!(
            t.lib().details().unwrap().last_verify_at.is_none(),
            "a cancelled verify records nothing"
        );
    }

    #[test]
    fn an_empty_library_verifies_to_an_empty_result() {
        let t = TempLib::new("empty");
        let result = t.verify().unwrap();
        assert_eq!(result, VerifyResult::default());
        assert_eq!(t.lib().details().unwrap().last_verify_problems, Some(0));
    }

    #[test]
    fn a_read_only_library_can_be_verified_but_remembers_nothing() {
        use std::io::{Seek, SeekFrom, Write};
        let mut t = TempLib::new("readonly");
        let a = t.add(20_000);
        unlock(&a.scan);
        std::fs::write(&a.scan, vec![9u8; 20_000]).unwrap();
        // A newer app version wrote this library: it opens read-only.
        t.lib = None;
        {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(t.dir.join(DB_FILE))
                .unwrap();
            file.seek(SeekFrom::Start(60)).unwrap();
            file.write_all(&(SCHEMA_VERSION + 1).to_be_bytes()).unwrap();
        }
        t.lib = Some(Library::open(&t.dir).unwrap());
        assert!(t.lib().is_read_only());
        let result = t.verify().unwrap();
        assert_eq!(result.problems.len(), 1);
        assert_eq!(result.problems[0].reason, VerifyReason::Changed);
        assert!(t.lib().details().unwrap().last_verify_at.is_none());
    }

    /// Hashing speed over one large original; run with
    /// `cargo test -p gtp-app --release -- --ignored --nocapture time_verify`.
    #[test]
    #[ignore = "timing run, not a check"]
    fn time_verify_of_a_large_original() {
        let t = TempLib::new("timing");
        let size = 512 * 1024 * 1024;
        t.add(size);
        let start = std::time::Instant::now();
        let result = t.verify().unwrap();
        let took = start.elapsed();
        eprintln!(
            "verified {} MB in {took:?}: {:.0} MB/s ({} problems)",
            result.bytes_checked / 1_000_000,
            result.bytes_checked as f64 / 1e6 / took.as_secs_f64(),
            result.problems.len()
        );
    }
}
