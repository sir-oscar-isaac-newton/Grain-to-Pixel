//! The capture job (H-08): takes one RGB triplet with the light and the camera and stores it as a
//! frame. One cancellable job per frame.
//!
//! 1. [`CaptureSequence`] lights red, green and blue in turn and collects the three NEF files (the
//!    light is off again when it ends, whichever way);
//! 2. the files are staged in a temporary folder (the merge reads EXIF from a file), merged, and
//!    stored by the same code as an imported triplet (`import_triplets::store_merged`): the same
//!    EXIF mismatch and clipping warnings, the same flat field, the converted planes as originals,
//!    the starting recipe and the thumbnail. The three files are stored directly with their known
//!    channels, not through the file grouping (ADR 0004, amendment 6);
//! 3. the temperature and supply warnings of the sequence are attached to the stored frame.
//!
//! A captured frame gets no colour balance (it stays a `RgbTriplet`, see P-22). A job that fails or
//! is cancelled stores nothing: no frame row, no files, no staging folder.

use std::path::{Path, PathBuf};
use std::time::Instant;

use gtp_capture::{CameraSource, CaptureSequence, CaptureWarning, CapturedFile, LightSource};
use gtp_core::{
    AppError, ErrorCode, FrameId, ImportResult, ImportWarning, ImportWarningKind, Look, RollId,
};
use gtp_ingest::{merge_triplet, read_exposure_info};
use gtp_io::{Limits, RawDecoder};

use super::capture_levels::{exposure_from_exif, CaptureInfo, LevelsUsed};
use super::import::{clean_up, report_roll_base};
use super::import_triplets::{store_merged, Target, TripletError};
use crate::jobs::{JobCtx, LibraryEvents};
use crate::services::counters;
use crate::services::library::Library;

/// The stage name reported while capturing.
pub const STAGE: &str = "capturing";

/// Steps of the job for progress: three pictures, then merging and storing (five steps, as an
/// import counts them).
const CAPTURE_STEPS: f32 = 3.0;
const STORE_STEPS: f32 = 5.0;

/// Captures one triplet into `roll`: runs `sequence` on `light` and `camera`, then stores the three
/// pictures as a frame, read by `decoder`. Returns an [`ImportResult`] with the one frame and its
/// warnings (the merge's, and the sequence's temperature and supply warnings). Ends with
/// `Cancelled` when cancelled, or with the error that stopped it; nothing is stored then. `used`
/// says which levels `sequence` lights: it is recorded with the frame, and the shots are compared
/// with the camera settings the saved levels were balanced on (a difference is a warning).
#[allow(clippy::too_many_arguments)]
pub fn capture_triplet(
    library: &Library,
    roll: RollId,
    sequence: &CaptureSequence<'_>,
    light: &mut dyn LightSource,
    camera: &mut dyn CameraSource,
    decoder: &dyn RawDecoder,
    ctx: &JobCtx,
    events: &dyn LibraryEvents,
    look: Look,
    used: &LevelsUsed,
) -> Result<ImportResult, AppError> {
    let before = library.read(|c| c.get_roll(roll))?;
    let started = Instant::now();
    let total = CAPTURE_STEPS + STORE_STEPS;
    ctx.progress(STAGE, 0.0);
    let mut taken = 0.0;
    let outcome = sequence.run(light, camera, &|| ctx.is_cancelled(), &mut |_| {
        taken += 1.0;
        ctx.progress(STAGE, taken / total);
    })?;
    ctx.check_cancelled()?;

    let frame_id = FrameId::new();
    let progress = |step: f32| ctx.progress(STAGE, (CAPTURE_STEPS + step) / total);
    let target = Target { roll, look };
    let stored = store_captured(
        library,
        target,
        &outcome.files,
        frame_id,
        decoder,
        ctx,
        &progress,
        used,
    );
    let (frame, mut warnings) = match stored {
        Ok(stored) => stored,
        Err(err) => {
            clean_up(library, roll, frame_id);
            return Err(err);
        }
    };
    warnings.extend(outcome.warnings.iter().map(warning_kind));

    let mut result = ImportResult::default();
    result.frames.push(frame.id);
    result
        .warnings
        .extend(warnings.into_iter().map(|kind| ImportWarning {
            frame_id: frame.id,
            kind,
        }));
    result
        .frame_ms
        .push(u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX));
    counters::count_import(library);
    events.library_changed(Some(roll));
    ctx.progress(STAGE, 1.0);
    report_roll_base(library, &before, &mut result)?;
    Ok(result)
}

/// The sequence's warning as the job result carries it.
fn warning_kind(warning: &CaptureWarning) -> ImportWarningKind {
    match *warning {
        CaptureWarning::HighTemperature { temp_c } => ImportWarningKind::HighTemperature {
            temp_c: temp_c.round().max(0.0) as u32,
        },
        CaptureWarning::LowSupply { vbus_mv } => ImportWarningKind::LowSupply {
            vbus_mv: u32::try_from(vbus_mv).unwrap_or(0),
        },
    }
}

/// Merges and stores exactly three captured files (red-, green-, blue-lit) as the frame
/// `frame_id`, with the warnings of the merge, and records how it was lit. On failure the caller
/// removes the frame's files; the staging folder is already gone.
#[allow(clippy::too_many_arguments)]
fn store_captured(
    library: &Library,
    target: Target,
    files: &[CapturedFile],
    frame_id: FrameId,
    decoder: &dyn RawDecoder,
    ctx: &JobCtx,
    progress: &dyn Fn(f32),
    used: &LevelsUsed,
) -> Result<(gtp_core::Frame, Vec<ImportWarningKind>), AppError> {
    if files.len() != 3 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "A triplet is three pictures: the red-, green- and blue-lit exposure.",
        )
        .with_details(format!("{} pictures", files.len())));
    }
    let staging = Staging::create(frame_id, files)?;
    let [r, g, b] = &staging.paths;
    let merged = merge_triplet(decoder, r, g, b, &Limits::default())?;
    let info = CaptureInfo::new(
        used,
        [r, g, b].map(|path| exposure_from_exif(read_exposure_info(path))),
    );
    let json = info.to_json()?;
    let (frame, mut warnings) = store_merged(
        library,
        target,
        merged,
        [r, g, b],
        frame_id,
        ctx,
        progress,
        Some(&json),
    )
    .map_err(|err| match err {
        TripletError::Stop(err) => err,
        TripletError::Skip(files) => {
            let first = files.into_iter().next();
            AppError::new(
                ErrorCode::Io,
                "A captured picture could not be stored. Try the frame again.",
            )
            .with_details(first.map_or_else(String::new, |f| {
                format!(
                    "{}: {:?} {}",
                    f.path,
                    f.reason,
                    f.details.unwrap_or_default()
                )
            }))
        }
    })?;
    if info.exposure_differs(used) {
        warnings.push(ImportWarningKind::ExposureDiffersFromLevels);
    }
    Ok((frame, warnings))
}

/// The captured files written to a temporary folder for the merge, removed when dropped.
struct Staging {
    dir: PathBuf,
    paths: [PathBuf; 3],
}

impl Staging {
    fn create(frame: FrameId, files: &[CapturedFile]) -> Result<Self, AppError> {
        let dir = std::env::temp_dir().join(format!("gtp-capture-{frame}"));
        std::fs::create_dir_all(&dir).map_err(|err| AppError::from_io(&err))?;
        // From here on the folder is removed by `Drop`, also when a write fails.
        let mut staging = Self {
            dir,
            paths: [PathBuf::new(), PathBuf::new(), PathBuf::new()],
        };
        for (i, (file, prefix)) in files.iter().zip(["r", "g", "b"]).enumerate() {
            let path = staging
                .dir
                .join(format!("{prefix}-{}", safe_name(&file.name)));
            std::fs::write(&path, &file.bytes).map_err(|err| AppError::from_io(&err))?;
            staging.paths[i] = path;
        }
        Ok(staging)
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_dir_all(&self.dir) {
            tracing::debug!("removing the capture staging folder failed: {err}");
        }
    }
}

/// A file name that is safe to create in the staging folder: no folders, no odd characters.
pub(super) fn safe_name(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '.') {
        "picture.nef".to_owned()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Duration;

    use gtp_capture::{
        fakes::{no_picture, FakeCamera, FakeLight, FakeLightHandle, Shot},
        CaptureProfile, LightCommand, LightStatus, Pause,
    };
    use gtp_core::testing::synthetic::{self, Params, Pattern};
    use gtp_core::{FrameKind, ImageF32, SourceRole, TripletChannel};
    use gtp_ingest::testing::tiff_with_exposure;
    use gtp_io::{sha256_file, Cfa, RawLinear};

    use super::*;
    use crate::jobs::CancelFlag;
    use crate::services::roll_frame::{roll_create, RollInput};

    const W: u32 = 200;
    const H: u32 = 140;

    /// No test waits: the sequence's pauses return at once.
    struct NoWait;

    impl Pause for NoWait {
        fn pause(&self, _: Duration) {}
    }

    /// Decodes the staged files by their first letter (r, g, b): the mosaic of that exposure.
    struct Decoder(HashMap<char, RawLinear>);

    impl RawDecoder for Decoder {
        fn decode(&self, path: &Path, _: &Limits) -> Result<RawLinear, AppError> {
            let first = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.chars().next())
                .unwrap_or('?');
            self.0
                .get(&first)
                .cloned()
                .ok_or_else(|| AppError::new(ErrorCode::UnsupportedFormat, "not a picture"))
        }
    }

    fn exposure(negative: &ImageF32, lit: usize) -> RawLinear {
        let (w, h) = (negative.width as usize, negative.height as usize);
        let mut data = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let value = if Cfa::RGGB.color_at(x, y) == lit {
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
        decoder: Decoder,
        light: FakeLight,
        light_handle: FakeLightHandle,
    }

    impl Setup {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "gtp-capture-job-{label}-{}-{}",
                std::process::id(),
                FrameId::new()
            ));
            let lib = Library::create(&dir.join("lib"), "Capture").unwrap();
            let roll = roll_create(
                &lib,
                RollInput {
                    name: "Camera".into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .id;
            let negative = synthetic::negative(
                7,
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
            let decoder = Decoder(
                [('r', 0), ('g', 1), ('b', 2)]
                    .into_iter()
                    .map(|(c, lit)| (c, exposure(&negative, lit)))
                    .collect(),
            );
            let light = FakeLight::new();
            Self {
                dir,
                lib: Some(lib),
                roll,
                decoder,
                light_handle: light.handle(),
                light,
            }
        }

        fn lib(&self) -> &Library {
            self.lib.as_ref().unwrap()
        }

        /// Three pictures with these exposure times (as (1, n)), ISO 200 and f/5.6.
        fn files(times: [u32; 3]) -> Vec<CapturedFile> {
            times
                .iter()
                .enumerate()
                .map(|(i, n)| CapturedFile {
                    name: format!("DSC_{i:04}.NEF"),
                    bytes: tiff_with_exposure((1, *n), 200, (56, 10)),
                })
                .collect()
        }

        fn camera(&self, files: Vec<CapturedFile>) -> FakeCamera {
            FakeCamera::with_files(files).linked_to(self.light_handle.clone())
        }

        fn run(
            &mut self,
            camera: &mut FakeCamera,
            cancel: Option<&CancelFlag>,
        ) -> Result<ImportResult, AppError> {
            let (ctx, flag) = JobCtx::standalone();
            if cancel.is_some_and(CancelFlag::is_cancelled) {
                flag.cancel();
            }
            let sequence = CaptureSequence::with_pause(CaptureProfile::default(), &NoWait)
                .reading_wait(Duration::from_millis(10));
            capture_triplet(
                self.lib.as_ref().unwrap(),
                self.roll,
                &sequence,
                &mut self.light,
                camera,
                &self.decoder,
                &ctx,
                &Events::default(),
                Look::Neutral,
                &LevelsUsed::manual(CaptureProfile::default().levels),
            )
        }

        fn frames(&self) -> usize {
            self.lib().read(|c| c.list_frames(self.roll)).unwrap().len()
        }

        /// Every file under the library's originals and thumbs folders.
        fn stored_files(&self) -> Vec<PathBuf> {
            let mut all = Vec::new();
            let mut stack = vec![
                self.lib().resolve(Path::new("originals")).unwrap(),
                self.lib().resolve(Path::new("thumbs")).unwrap(),
            ];
            while let Some(dir) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else {
                        all.push(path);
                    }
                }
            }
            all
        }

        fn last_levels(&self) -> [u8; 3] {
            self.light_handle.levels()
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
    }

    impl LibraryEvents for Events {
        fn library_changed(&self, _: Option<RollId>) {
            *self.changed.lock().unwrap() += 1;
        }
    }

    fn is_staging_left(frame_hint: &str) -> bool {
        std::fs::read_dir(std::env::temp_dir())
            .map(|entries| {
                entries.flatten().any(|e| {
                    e.file_name().to_string_lossy().starts_with("gtp-capture-")
                        && e.file_name().to_string_lossy().contains(frame_hint)
                })
            })
            .unwrap_or(false)
    }

    #[test]
    fn the_whole_job_stores_a_triplet_frame_with_three_hashed_single_channel_sources() {
        let mut s = Setup::new("whole");
        let mut camera = s.camera(Setup::files([60, 60, 60]));
        let result = s.run(&mut camera, None).unwrap();
        assert_eq!(result.frames.len(), 1);
        assert!(result.skipped.is_empty());
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert_eq!(result.frame_ms.len(), 1);

        let id = result.frames[0];
        let frame = s.lib().read(|c| c.get_frame(id)).unwrap();
        assert_eq!(frame.kind, FrameKind::RgbTriplet);
        assert_eq!((frame.width, frame.height, frame.has_ir), (W, H, false));
        let sources = s.lib().read(|c| c.sources_of(id)).unwrap();
        assert_eq!(sources.len(), 3);
        for (role, name) in [
            (SourceRole::R, "r.tif"),
            (SourceRole::G, "g.tif"),
            (SourceRole::B, "b.tif"),
        ] {
            let of_role: Vec<_> = sources.iter().filter(|s| s.role == role).collect();
            assert_eq!(of_role.len(), 1, "{role:?}");
            assert!(of_role[0].rel_path.ends_with(name), "{role:?}");
        }
        for source in &sources {
            let path = s.lib().resolve(Path::new(&source.rel_path)).unwrap();
            assert!(std::fs::metadata(&path).unwrap().permissions().readonly());
            assert_eq!(sha256_file(&path).unwrap(), source.sha256);
        }
        assert!(s
            .lib()
            .resolve(&crate::library_fs::thumb_rel(id))
            .unwrap()
            .is_file());
        // The light was lit red, green, blue and ended off.
        assert_eq!(s.last_levels(), [0, 0, 0]);
        assert!(!is_staging_left(&id.to_string()));
    }

    #[test]
    fn a_captured_frame_has_no_colour_balance_and_the_roll_stores_none() {
        let mut s = Setup::new("balance");
        let mut camera = s.camera(Setup::files([60, 60, 60]));
        let result = s.run(&mut camera, None).unwrap();
        let recipe = s
            .lib()
            .read(|c| c.load_recipe(result.frames[0]))
            .unwrap()
            .unwrap();
        assert_eq!(
            [
                recipe.color.red_cyan,
                recipe.color.green_magenta,
                recipe.color.blue_yellow
            ],
            [0.0; 3]
        );
        assert_eq!(
            s.lib().read(|c| c.get_roll(s.roll)).unwrap().color_balance,
            None
        );
    }

    #[test]
    fn mismatched_exposures_and_a_clipped_channel_are_warned_about_for_the_frame() {
        let mut s = Setup::new("warn");
        // The green shot used another shutter time.
        let mut camera = s.camera(Setup::files([60, 30, 60]));
        // Clip the red-lit exposure: every red photosite at the white level.
        let mut red = s.decoder.0[&'r'].clone();
        for y in 0..H as usize {
            for x in 0..W as usize {
                if Cfa::RGGB.color_at(x, y) == 0 {
                    red.data[y * W as usize + x] = 10_000;
                }
            }
        }
        s.decoder.0.insert('r', red);
        let result = s.run(&mut camera, None).unwrap();
        assert_eq!(result.frames.len(), 1, "warnings do not stop the frame");
        let kinds: Vec<_> = result.warnings.iter().map(|w| w.kind).collect();
        assert!(kinds.contains(&ImportWarningKind::ExposureTimeMismatch));
        assert!(kinds.iter().any(|k| matches!(
            k,
            ImportWarningKind::Clipped {
                channel: TripletChannel::Red,
                per_mille
            } if *per_mille > 900
        )));
        assert!(result
            .warnings
            .iter()
            .all(|w| w.frame_id == result.frames[0]));
    }

    #[test]
    fn a_warm_light_and_a_low_supply_are_attached_to_the_stored_frame() {
        let mut s = Setup::new("hot");
        s.light_handle.set_status(LightStatus {
            temp_c: Some(47.4),
            vbus_mv: Some(4700),
            connected: true,
        });
        let mut camera = s.camera(Setup::files([60, 60, 60]));
        let result = s.run(&mut camera, None).unwrap();
        let kinds: Vec<_> = result.warnings.iter().map(|w| w.kind).collect();
        assert_eq!(
            kinds,
            [
                ImportWarningKind::HighTemperature { temp_c: 47 },
                ImportWarningKind::LowSupply { vbus_mv: 4700 }
            ]
        );
        assert_eq!(result.warnings[0].frame_id, result.frames[0]);
    }

    #[test]
    fn a_temperature_abort_stores_nothing_and_leaves_the_light_off() {
        let mut s = Setup::new("abort");
        s.light_handle.set_status(LightStatus {
            temp_c: Some(56.0),
            vbus_mv: Some(5150),
            connected: true,
        });
        let mut camera = s.camera(Setup::files([60, 60, 60]));
        let err = s.run(&mut camera, None).unwrap_err();
        assert!(err.message.contains("too hot"), "{err}");
        assert_eq!(s.frames(), 0);
        assert!(s.stored_files().is_empty(), "{:?}", s.stored_files());
        assert_eq!(s.last_levels(), [0, 0, 0]);
    }

    #[test]
    fn a_camera_timeout_stores_nothing_and_leaves_the_light_off() {
        let mut s = Setup::new("timeout");
        let mut camera = s.camera(vec![]);
        let mut shots = FakeCamera::new(vec![
            Shot::File(Setup::files([60, 60, 60]).remove(0)),
            Shot::Fail(no_picture()),
        ])
        .linked_to(s.light_handle.clone());
        let err = s.run(&mut shots, None).unwrap_err();
        assert_eq!(err, no_picture());
        assert_eq!(s.frames(), 0);
        assert!(s.stored_files().is_empty());
        assert_eq!(s.last_levels(), [0, 0, 0]);
        // An empty camera fails the same way.
        assert_eq!(s.run(&mut camera, None).unwrap_err(), no_picture());
    }

    #[test]
    fn pictures_that_cannot_be_read_leave_no_frame_files_or_staging_folder() {
        let mut s = Setup::new("unreadable");
        s.decoder.0.remove(&'g');
        let mut camera = s.camera(Setup::files([60, 60, 60]));
        let err = s.run(&mut camera, None).unwrap_err();
        assert_eq!(err.code, ErrorCode::UnsupportedFormat);
        assert_eq!(s.frames(), 0);
        assert!(s.stored_files().is_empty());
        assert_eq!(s.last_levels(), [0, 0, 0]);
    }

    #[test]
    fn a_cancelled_job_stores_nothing_and_the_light_is_off() {
        let mut s = Setup::new("cancel");
        let flag = CancelFlag::default();
        flag.cancel();
        let mut camera = s.camera(Setup::files([60, 60, 60]));
        let err = s.run(&mut camera, Some(&flag)).unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        assert_eq!(s.frames(), 0);
        assert!(s.stored_files().is_empty());
        assert!(!s
            .light_handle
            .commands()
            .iter()
            .any(|c| matches!(c, LightCommand::SetLevels(l) if *l != [0; 3])));
    }

    #[test]
    fn a_cancel_while_storing_removes_the_partial_frame() {
        let s = Setup::new("cancel-store");
        let (ctx, flag) = JobCtx::standalone();
        let files = Setup::files([60, 60, 60]);
        let frame_id = FrameId::new();
        // The cancel arrives after the originals were written (at the first progress report that
        // follows them).
        let progress = |step: f32| {
            if step >= 3.0 {
                flag.cancel();
            }
        };
        let target = Target {
            roll: s.roll,
            look: Look::Neutral,
        };
        let err = store_captured(
            s.lib(),
            target,
            &files,
            frame_id,
            &s.decoder,
            &ctx,
            &progress,
            &LevelsUsed::manual([44, 22, 2]),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        clean_up(s.lib(), s.roll, frame_id);
        assert_eq!(s.frames(), 0);
        assert!(s.stored_files().is_empty());
        assert!(!is_staging_left(&frame_id.to_string()));
    }

    #[test]
    fn two_or_four_pictures_never_store_a_frame() {
        let s = Setup::new("count");
        let (ctx, _) = JobCtx::standalone();
        let target = Target {
            roll: s.roll,
            look: Look::Neutral,
        };
        for count in [0usize, 2, 4] {
            let mut files = Setup::files([60, 60, 60]);
            files.push(Setup::files([60, 60, 60]).remove(0));
            files.truncate(count);
            let err = store_captured(
                s.lib(),
                target,
                &files,
                FrameId::new(),
                &s.decoder,
                &ctx,
                &|_| {},
                &LevelsUsed::manual([44, 22, 2]),
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{count} pictures");
        }
        assert_eq!(s.frames(), 0);
        assert!(s.stored_files().is_empty());
    }

    #[test]
    fn a_picture_name_cannot_leave_the_staging_folder() {
        assert_eq!(safe_name("DSC_0001.NEF"), "DSC_0001.NEF");
        assert_eq!(safe_name("../../evil.nef"), "evil.nef");
        assert_eq!(safe_name("a b:c.nef"), "a_b_c.nef");
        assert_eq!(safe_name(""), "picture.nef");
        assert_eq!(safe_name(".."), "picture.nef");
    }
}
