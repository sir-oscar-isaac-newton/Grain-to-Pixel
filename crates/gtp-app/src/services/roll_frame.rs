//! Rolls and frames of the open library (L-04). Reads use the reader connection, writes the writer.
//!
//! Deleting is database first, files second: the rows are deleted in one transaction, and only
//! after it commits are the frames' originals and thumbnails removed. If the transaction fails, no
//! file is touched. Originals are read-only on disk, so the flag is cleared before removal; a file
//! that still cannot be removed is reported in [`DeleteReport::files_left`] (and logged by the
//! caller), never silently kept.

use std::path::{Path, PathBuf};

use gtp_core::{
    AppError, ErrorCode, FilmFormat, Frame, FrameId, FrameKind, ImageF32, Roll, RollId,
};
use gtp_negative::{auto_film_base, BaseEstimate};
use serde::{Deserialize, Serialize};

use crate::library_fs::{originals_rel, thumb_rel, ORIGINALS_DIR};
use crate::paths::display_path;
use crate::pipeline::{build_proxy, PREVIEW_EDGE};
use crate::services::library::Library;

/// Most frames one page may hold.
pub const MAX_PAGE_SIZE: u32 = 500;

/// The editable fields of a roll, used both to create and to update one (an update replaces them
/// all; an empty text clears its field).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct RollInput {
    pub name: String,
    pub film_stock: Option<String>,
    pub camera: Option<String>,
    pub iso_box: Option<u32>,
    pub iso_shot: Option<u32>,
    pub shot_from: Option<String>,
    pub shot_to: Option<String>,
    pub notes: String,
}

impl RollInput {
    fn into_roll(
        self,
        id: RollId,
        film_base: (Option<[f32; 3]>, Option<f32>),
        created_at: String,
    ) -> Roll {
        Roll {
            id,
            name: self.name,
            film_stock: self.film_stock,
            format: FilmFormat::Mm35,
            camera: self.camera,
            iso_box: self.iso_box,
            iso_shot: self.iso_shot,
            shot_from: self.shot_from,
            shot_to: self.shot_to,
            notes: self.notes,
            film_base_rgb: film_base.0,
            film_base_confidence: film_base.1,
            created_at,
        }
    }
}

/// Which frames to list: one roll's or all, a page at a time (pages count from 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FrameQuery {
    pub roll_id: Option<RollId>,
    pub page: u32,
    pub page_size: u32,
}

/// What the grid needs of a frame.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FrameSummary {
    pub id: FrameId,
    pub roll_id: RollId,
    pub index_in_roll: u32,
    pub kind: FrameKind,
    pub width: u32,
    pub height: u32,
    pub has_ir: bool,
    pub note: String,
    pub edited_at: Option<String>,
    /// True when the frame's thumbnail file exists.
    pub has_thumb: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FramePage {
    pub items: Vec<FrameSummary>,
    /// Frames matching the query, over all pages.
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

/// The outcome of a delete: rows removed, and files that could not be removed afterwards.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DeleteReport {
    pub frames_deleted: u32,
    pub files_left: Vec<String>,
}

pub fn roll_create(library: &Library, input: RollInput) -> Result<Roll, AppError> {
    let roll = input.into_roll(RollId::new(), (None, None), String::new());
    library.write(|c| c.insert_roll(&roll))
}

/// Replaces a roll's editable fields; its film base (with its confidence) and creation time are kept.
pub fn roll_update(library: &Library, id: RollId, input: RollInput) -> Result<Roll, AppError> {
    library.write(|c| {
        let current = c.get_roll(id)?;
        let base = (current.film_base_rgb, current.film_base_confidence);
        c.update_roll(&input.into_roll(id, base, current.created_at))
    })
}

pub fn roll_list(library: &Library) -> Result<Vec<Roll>, AppError> {
    library.read(|c| c.list_rolls())
}

/// Sets (or with `None` clears) the roll's film base by hand; every channel must be finite and above
/// 0. A base set by hand has no confidence, so it is never flagged as uncertain.
pub fn roll_set_film_base(
    library: &Library,
    id: RollId,
    base: Option<[f32; 3]>,
) -> Result<Roll, AppError> {
    if let Some(rgb) = base {
        if !rgb.iter().all(|v| v.is_finite() && *v > 0.0) {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "A film base needs three values above 0.",
            )
            .with_details(format!("{rgb:?}")));
        }
    }
    library.write(|c| {
        let mut roll = c.get_roll(id)?;
        roll.film_base_rgb = base;
        roll.film_base_confidence = None;
        c.update_roll(&roll)
    })
}

/// The roll's film base after [`ensure_roll_base`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnsuredBase {
    /// The base to use for every frame of the roll.
    pub rgb: [f32; 3],
    /// Confidence of the automatic measurement; `None` for a base set by hand.
    pub confidence: Option<f32>,
    /// True when this call stored the base (the roll had none).
    pub stored_now: bool,
}

/// Makes sure the roll has a film base. When it has none, `estimate` (measured on a frame of this
/// roll) is stored with its confidence, so later frames share it; when it already has one, set by
/// hand or by an earlier frame, that one is returned and never overwritten. A low-confidence
/// estimate is stored too, and flagged by [`Roll::film_base_uncertain`] for the UI.
///
/// This is the one function import, scan and capture call after analyzing a roll's frame.
pub fn ensure_roll_base(
    library: &Library,
    roll: RollId,
    estimate: BaseEstimate,
) -> Result<EnsuredBase, AppError> {
    let (stored, stored_now) = library
        .write(|c| c.set_roll_film_base_if_unset(roll, estimate.rgb, Some(estimate.confidence)))?;
    let rgb = stored
        .film_base_rgb
        .ok_or_else(|| AppError::internal("the roll has no film base right after it was stored"))?;
    Ok(EnsuredBase {
        rgb,
        confidence: stored.film_base_confidence,
        stored_now,
    })
}

/// [`ensure_roll_base`] for a frame that has not been analyzed: measures the film base on a
/// preview-size copy of `image` (a full-frame scan, rebate included).
pub fn ensure_roll_base_from_image(
    library: &Library,
    roll: RollId,
    image: &ImageF32,
) -> Result<EnsuredBase, AppError> {
    let proxy = build_proxy(image, PREVIEW_EDGE)?;
    ensure_roll_base(library, roll, auto_film_base(&proxy)?)
}

/// Deletes a roll with all its frames, then their files.
pub fn roll_delete(library: &Library, id: RollId) -> Result<DeleteReport, AppError> {
    let frames = library.write(|c| {
        let frames = c.list_frames(id)?;
        c.delete_roll(id)?;
        Ok(frames)
    })?;
    let mut report = DeleteReport {
        frames_deleted: u32::try_from(frames.len()).unwrap_or(u32::MAX),
        files_left: Vec::new(),
    };
    for frame in &frames {
        remove_in_library(library, &thumb_rel(frame.id), &mut report.files_left);
    }
    remove_in_library(
        library,
        &Path::new(ORIGINALS_DIR).join(id.to_string()),
        &mut report.files_left,
    );
    Ok(report)
}

/// The row offset of page number page (counting from 0), after checking the page size.
pub(crate) fn page_offset(page: u32, page_size: u32) -> Result<u32, AppError> {
    if !(1..=MAX_PAGE_SIZE).contains(&page_size) {
        return Err(
            AppError::new(ErrorCode::InvalidInput, "The page size is out of range.")
                .with_details(format!("{page_size} (1 to {MAX_PAGE_SIZE})")),
        );
    }
    page.checked_mul(page_size)
        .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "That page is out of range."))
}

pub fn frame_list(library: &Library, query: FrameQuery) -> Result<FramePage, AppError> {
    let offset = page_offset(query.page, query.page_size)?;
    let (frames, total) = library.read(|c| {
        Ok((
            c.list_frames_page(query.roll_id, offset, query.page_size)?,
            c.count_frames(query.roll_id)?,
        ))
    })?;
    let items = frames.into_iter().map(|f| summary(library, f)).collect();
    Ok(FramePage {
        items,
        total,
        page: query.page,
        page_size: query.page_size,
    })
}

/// Largest thumbnail file read (thumbnails are about 512 px; this is a guard against a wrong file).
const MAX_THUMB_BYTES: u64 = 4 * 1024 * 1024;

/// The frame's thumbnail (the JPEG file) as bytes, for the grid. `NotFound` when it has none yet.
pub fn frame_thumbnail(library: &Library, id: FrameId) -> Result<Vec<u8>, AppError> {
    let path = library.resolve(&thumb_rel(id))?;
    let size = std::fs::metadata(&path)
        .map_err(|err| AppError::from_io(&err))?
        .len();
    if size > MAX_THUMB_BYTES {
        return Err(
            AppError::new(ErrorCode::InvalidInput, "That thumbnail file is too large.")
                .with_details(format!("{size} bytes")),
        );
    }
    std::fs::read(&path).map_err(|err| AppError::from_io(&err))
}

pub fn frame_get(library: &Library, id: FrameId) -> Result<Frame, AppError> {
    library.read(|c| c.get_frame(id))
}

pub fn frame_set_note(library: &Library, id: FrameId, note: &str) -> Result<Frame, AppError> {
    library.write(|c| c.set_frame_note(id, note))
}

/// Deletes frames (unknown ids are skipped), then their originals and thumbnails.
pub fn frame_delete(library: &Library, ids: &[FrameId]) -> Result<DeleteReport, AppError> {
    let (frames, deleted) = library.write(|c| {
        let frames: Vec<Frame> = ids.iter().filter_map(|id| c.get_frame(*id).ok()).collect();
        let deleted = c.delete_frames(ids)?;
        Ok((frames, deleted))
    })?;
    let mut report = DeleteReport {
        frames_deleted: u32::try_from(deleted).unwrap_or(u32::MAX),
        files_left: Vec::new(),
    };
    for frame in &frames {
        remove_in_library(
            library,
            &originals_rel(frame.roll_id, frame.id),
            &mut report.files_left,
        );
        remove_in_library(library, &thumb_rel(frame.id), &mut report.files_left);
    }
    Ok(report)
}

pub(crate) fn summary(library: &Library, f: Frame) -> FrameSummary {
    let has_thumb = library.resolve(&thumb_rel(f.id)).is_ok_and(|p| p.is_file());
    FrameSummary {
        id: f.id,
        roll_id: f.roll_id,
        index_in_roll: f.index_in_roll,
        kind: f.kind,
        width: f.width,
        height: f.height,
        has_ir: f.has_ir,
        note: f.note,
        edited_at: f.edited_at,
        has_thumb,
    }
}

/// Removes a file or folder inside the library, adding what could not be removed to `left`.
fn remove_in_library(library: &Library, rel: &Path, left: &mut Vec<String>) {
    match library.resolve(rel) {
        Ok(path) => remove_tree(&path, left),
        Err(_) => left.push(rel.display().to_string()),
    }
}

/// Removes `path` (file or folder, read-only files included); missing is fine.
pub(crate) fn remove_tree(path: &Path, left: &mut Vec<String>) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        let entries: Vec<PathBuf> = match std::fs::read_dir(path) {
            Ok(entries) => entries.filter_map(|e| e.ok().map(|e| e.path())).collect(),
            Err(_) => {
                left.push(display_path(path));
                return;
            }
        };
        for entry in entries {
            remove_tree(&entry, left);
        }
        if std::fs::remove_dir(path).is_err() {
            left.push(display_path(path));
        }
        return;
    }
    let mut permissions = meta.permissions();
    if permissions.readonly() {
        // Originals are marked read-only on import; Windows refuses to delete them otherwise.
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        let _ = std::fs::set_permissions(path, permissions);
    }
    if std::fs::remove_file(path).is_err() {
        left.push(display_path(path));
    }
}

#[cfg(test)]
mod tests {
    use gtp_catalog::SCHEMA_VERSION;

    use super::*;
    use crate::library_fs::DB_FILE;

    struct TempLib {
        dir: PathBuf,
        lib: Option<Library>,
    }

    impl TempLib {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "gtp-rollframe-{label}-{}-{}",
                std::process::id(),
                FrameId::new()
            ));
            let lib = Library::create(&dir, "Test").unwrap();
            Self {
                dir,
                lib: Some(lib),
            }
        }

        fn lib(&self) -> &Library {
            self.lib.as_ref().unwrap()
        }

        fn reopen(&mut self) {
            self.lib = None;
            self.lib = Some(Library::open(&self.dir).unwrap());
        }
    }

    impl Drop for TempLib {
        fn drop(&mut self) {
            self.lib = None;
            let _ = remove_all_for_test(&self.dir);
        }
    }

    fn remove_all_for_test(dir: &Path) -> std::io::Result<()> {
        let mut left = Vec::new();
        remove_tree(dir, &mut left);
        Ok(())
    }

    fn input(name: &str) -> RollInput {
        RollInput {
            name: name.into(),
            film_stock: Some("Ilford HP5".into()),
            camera: Some("Leica M6".into()),
            iso_box: Some(400),
            iso_shot: Some(1600),
            shot_from: Some("2026-09".into()),
            notes: "pushed two stops".into(),
            ..RollInput::default()
        }
    }

    /// Adds a frame with a read-only original and a thumbnail on disk.
    fn frame_with_files(lib: &Library, roll: RollId, index: u32) -> Frame {
        let frame = Frame {
            id: FrameId::new(),
            roll_id: roll,
            index_in_roll: index,
            kind: FrameKind::ScanRgbIr,
            width: 100,
            height: 66,
            has_ir: true,
            note: String::new(),
            created_at: String::new(),
            edited_at: None,
        };
        let stored = lib.write(|c| c.insert_frame(&frame)).unwrap();
        let dir = lib.resolve(&originals_rel(roll, frame.id)).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let original = dir.join("scan.tif");
        std::fs::write(&original, b"tiff").unwrap();
        let mut perms = std::fs::metadata(&original).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&original, perms).unwrap();
        std::fs::write(lib.resolve(&thumb_rel(frame.id)).unwrap(), b"jpg").unwrap();
        stored
    }

    fn estimate(rgb: [f32; 3], confidence: f32) -> BaseEstimate {
        BaseEstimate { rgb, confidence }
    }

    #[test]
    fn the_first_estimate_sets_the_roll_base_and_a_later_one_never_overwrites_it() {
        let t = TempLib::new("ensure");
        let roll = roll_create(t.lib(), input("R")).unwrap();
        assert!(roll.film_base_rgb.is_none() && roll.film_base_confidence.is_none());

        let first = ensure_roll_base(t.lib(), roll.id, estimate([0.8, 0.5, 0.3], 0.9)).unwrap();
        assert_eq!(
            first,
            EnsuredBase {
                rgb: [0.8, 0.5, 0.3],
                confidence: Some(0.9),
                stored_now: true
            }
        );
        let second = ensure_roll_base(t.lib(), roll.id, estimate([0.1, 0.1, 0.1], 0.99)).unwrap();
        assert_eq!(second.rgb, [0.8, 0.5, 0.3], "the first frame's base stays");
        assert!(!second.stored_now);
        let stored = t.lib().read(|c| c.get_roll(roll.id)).unwrap();
        assert_eq!(stored.film_base_rgb, Some([0.8, 0.5, 0.3]));
        assert_eq!(stored.film_base_confidence, Some(0.9));
        assert!(!stored.film_base_uncertain());
        // Editing the roll keeps the base and its confidence.
        let edited = roll_update(t.lib(), roll.id, input("Renamed")).unwrap();
        assert_eq!(edited.film_base_confidence, Some(0.9));
    }

    #[test]
    fn a_base_set_by_hand_is_left_untouched_and_has_no_confidence() {
        let t = TempLib::new("ensure-manual");
        let roll = roll_create(t.lib(), input("R")).unwrap();
        roll_set_film_base(t.lib(), roll.id, Some([0.7, 0.4, 0.2])).unwrap();
        let ensured = ensure_roll_base(t.lib(), roll.id, estimate([0.9, 0.9, 0.9], 0.95)).unwrap();
        assert_eq!(ensured.rgb, [0.7, 0.4, 0.2]);
        assert_eq!(ensured.confidence, None);
        assert!(!ensured.stored_now);

        // Picking by hand replaces an automatic base and drops its confidence.
        let auto = roll_create(t.lib(), input("Auto")).unwrap();
        ensure_roll_base(t.lib(), auto.id, estimate([0.8, 0.5, 0.3], 0.2)).unwrap();
        assert!(t
            .lib()
            .read(|c| c.get_roll(auto.id))
            .unwrap()
            .film_base_uncertain());
        let by_hand = roll_set_film_base(t.lib(), auto.id, Some([0.75, 0.5, 0.3])).unwrap();
        assert_eq!(by_hand.film_base_confidence, None);
        assert!(!by_hand.film_base_uncertain());
        // Clearing the base lets the next frame measure it again.
        roll_set_film_base(t.lib(), auto.id, None).unwrap();
        let again = ensure_roll_base(t.lib(), auto.id, estimate([0.6, 0.6, 0.6], 0.8)).unwrap();
        assert!(again.stored_now);
    }

    #[test]
    fn a_low_confidence_estimate_is_stored_and_flagged() {
        let t = TempLib::new("ensure-low");
        let roll = roll_create(t.lib(), input("R")).unwrap();
        let ensured = ensure_roll_base(t.lib(), roll.id, estimate([0.8, 0.5, 0.3], 0.2)).unwrap();
        assert!(ensured.stored_now);
        let stored = t.lib().read(|c| c.get_roll(roll.id)).unwrap();
        assert_eq!(stored.film_base_confidence, Some(0.2));
        assert!(stored.film_base_uncertain());
    }

    #[test]
    fn bad_estimates_and_unknown_rolls_are_rejected() {
        let t = TempLib::new("ensure-bad");
        let roll = roll_create(t.lib(), input("R")).unwrap();
        for bad in [
            estimate([0.0, 0.5, 0.3], 0.9),
            estimate([f32::NAN, 0.5, 0.3], 0.9),
            estimate([0.8, 0.5, 0.3], 1.5),
            estimate([0.8, 0.5, 0.3], f32::NAN),
        ] {
            let err = ensure_roll_base(t.lib(), roll.id, bad).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{bad:?}");
        }
        assert!(t
            .lib()
            .read(|c| c.get_roll(roll.id))
            .unwrap()
            .film_base_rgb
            .is_none());
        let err = ensure_roll_base(t.lib(), RollId::new(), estimate([0.8, 0.5, 0.3], 0.9));
        assert_eq!(err.unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn the_base_can_be_measured_from_an_image() {
        use gtp_core::testing::synthetic::{self, Params};
        let t = TempLib::new("ensure-image");
        let roll = roll_create(t.lib(), input("R")).unwrap();
        let s = synthetic::negative(
            3,
            &Params {
                width: 300,
                height: 200,
                rebate: [20; 4],
                ..Params::default()
            },
        )
        .unwrap();
        let ensured = ensure_roll_base_from_image(t.lib(), roll.id, &s.image).unwrap();
        assert!(ensured.stored_now);
        for c in 0..3 {
            let truth = s.truth.base[c];
            assert!((ensured.rgb[c] - truth).abs() / truth < 0.02, "channel {c}");
        }
        assert!(ensured.confidence.is_some_and(|c| c > 0.5));
    }

    #[test]
    fn rolls_are_created_updated_and_listed() {
        let t = TempLib::new("rolls");
        let roll = roll_create(t.lib(), input("Autumn")).unwrap();
        assert_eq!(roll.iso_shot, Some(1600));
        let based = roll_set_film_base(t.lib(), roll.id, Some([0.8, 0.5, 0.3])).unwrap();

        let mut edit = input("Autumn walk");
        edit.camera = None;
        let updated = roll_update(t.lib(), roll.id, edit).unwrap();
        assert_eq!(updated.name, "Autumn walk");
        assert_eq!(updated.camera, None, "an update replaces every field");
        assert_eq!(
            updated.film_base_rgb, based.film_base_rgb,
            "the film base is kept"
        );
        assert_eq!(updated.created_at, roll.created_at);
        assert_eq!(roll_list(t.lib()).unwrap(), vec![updated]);

        let cleared = roll_set_film_base(t.lib(), roll.id, None).unwrap();
        assert!(cleared.film_base_rgb.is_none());
        for bad in [[0.0, 0.5, 0.3], [f32::NAN, 0.5, 0.3], [-1.0, 0.5, 0.3]] {
            let err = roll_set_film_base(t.lib(), roll.id, Some(bad)).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
        }
        let err = roll_update(t.lib(), RollId::new(), input("x")).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        let err = roll_create(t.lib(), RollInput::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput, "a roll needs a name");
    }

    #[test]
    fn a_thumbnail_comes_back_as_its_bytes_or_not_found() {
        let t = TempLib::new("thumb-bytes");
        let roll = roll_create(t.lib(), input("R")).unwrap();
        let with = frame_with_files(t.lib(), roll.id, 1);
        assert_eq!(frame_thumbnail(t.lib(), with.id).unwrap(), b"jpg");
        let none = frame_thumbnail(t.lib(), FrameId::new()).unwrap_err();
        assert_eq!(none.code, ErrorCode::NotFound);
        // A file that is not a plausible thumbnail is refused rather than sent whole.
        let big = t.lib().resolve(&thumb_rel(with.id)).unwrap();
        std::fs::write(&big, vec![0u8; (MAX_THUMB_BYTES + 1) as usize]).unwrap();
        let err = frame_thumbnail(t.lib(), with.id).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn frames_page_with_totals_and_thumbnail_flags() {
        let t = TempLib::new("pages");
        let roll = roll_create(t.lib(), input("R")).unwrap();
        let with_files = frame_with_files(t.lib(), roll.id, 1);
        for i in 2..=5 {
            t.lib()
                .write(|c| {
                    c.insert_frame(&Frame {
                        id: FrameId::new(),
                        index_in_roll: i,
                        ..with_files.clone()
                    })
                })
                .unwrap();
        }
        let query = |page, page_size| FrameQuery {
            roll_id: Some(roll.id),
            page,
            page_size,
        };
        let first = frame_list(t.lib(), query(0, 2)).unwrap();
        assert_eq!(first.total, 5);
        assert_eq!(first.items.len(), 2);
        assert_eq!(first.items[0].id, with_files.id);
        assert!(first.items[0].has_thumb && !first.items[1].has_thumb);
        let last = frame_list(t.lib(), query(2, 2)).unwrap();
        assert_eq!(last.items.len(), 1);
        assert_eq!(last.items[0].index_in_roll, 5);
        for bad in [query(0, 0), query(0, MAX_PAGE_SIZE + 1), query(u32::MAX, 2)] {
            assert_eq!(
                frame_list(t.lib(), bad).unwrap_err().code,
                ErrorCode::InvalidInput
            );
        }
        let noted = frame_set_note(t.lib(), with_files.id, "  lighthouse ").unwrap();
        assert_eq!(noted.note, "lighthouse");
        assert_eq!(
            frame_get(t.lib(), with_files.id).unwrap().note,
            "lighthouse"
        );
    }

    #[test]
    fn deleting_frames_removes_their_read_only_files_after_the_commit() {
        let t = TempLib::new("delete-frames");
        let roll = roll_create(t.lib(), input("R")).unwrap();
        let gone = frame_with_files(t.lib(), roll.id, 1);
        let kept = frame_with_files(t.lib(), roll.id, 2);
        let report = frame_delete(t.lib(), &[gone.id, FrameId::new()]).unwrap();
        assert_eq!(
            report,
            DeleteReport {
                frames_deleted: 1,
                files_left: vec![]
            }
        );
        assert!(!t
            .lib()
            .resolve(&originals_rel(roll.id, gone.id))
            .unwrap()
            .exists());
        assert!(!t.lib().resolve(&thumb_rel(gone.id)).unwrap().exists());
        assert!(t
            .lib()
            .resolve(&originals_rel(roll.id, kept.id))
            .unwrap()
            .exists());
        assert_eq!(
            frame_get(t.lib(), gone.id).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn deleting_a_roll_removes_its_folder_and_thumbnails() {
        let t = TempLib::new("delete-roll");
        let roll = roll_create(t.lib(), input("R")).unwrap();
        let f1 = frame_with_files(t.lib(), roll.id, 1);
        let f2 = frame_with_files(t.lib(), roll.id, 2);
        let report = roll_delete(t.lib(), roll.id).unwrap();
        assert_eq!(report.frames_deleted, 2);
        assert!(report.files_left.is_empty(), "{:?}", report.files_left);
        let roll_dir = t
            .lib()
            .resolve(&Path::new(ORIGINALS_DIR).join(roll.id.to_string()));
        assert!(!roll_dir.unwrap().exists());
        for f in [f1, f2] {
            assert!(!t.lib().resolve(&thumb_rel(f.id)).unwrap().exists());
        }
        assert!(roll_list(t.lib()).unwrap().is_empty());
        assert_eq!(
            roll_delete(t.lib(), roll.id).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_failed_delete_leaves_every_file_in_place() {
        let mut t = TempLib::new("delete-fails");
        let roll = roll_create(t.lib(), input("R")).unwrap();
        let frame = frame_with_files(t.lib(), roll.id, 1);
        // A newer app version wrote the library: it reopens read-only and every write fails.
        t.lib = None;
        set_schema_version(&t.dir.join(DB_FILE), SCHEMA_VERSION + 1);
        t.reopen();
        let err = frame_delete(t.lib(), &[frame.id]).unwrap_err();
        assert_eq!(err.code, ErrorCode::LibraryTooNew);
        let err = roll_delete(t.lib(), roll.id).unwrap_err();
        assert_eq!(err.code, ErrorCode::LibraryTooNew);
        assert!(t
            .lib()
            .resolve(&originals_rel(roll.id, frame.id))
            .unwrap()
            .join("scan.tif")
            .exists());
        assert!(t.lib().resolve(&thumb_rel(frame.id)).unwrap().exists());
        assert_eq!(
            frame_get(t.lib(), frame.id).unwrap().id,
            frame.id,
            "reads still work"
        );
    }

    #[test]
    fn a_file_that_cannot_be_removed_is_reported() {
        let t = TempLib::new("files-left");
        let roll = roll_create(t.lib(), input("R")).unwrap();
        let frame = frame_with_files(t.lib(), roll.id, 1);
        let original = t
            .lib()
            .resolve(&originals_rel(roll.id, frame.id))
            .unwrap()
            .join("scan.tif");
        let _blocker = block_removal(&original);
        let report = frame_delete(t.lib(), &[frame.id]).unwrap();
        assert_eq!(report.frames_deleted, 1, "the rows are gone either way");
        assert!(
            report.files_left.iter().any(|f| f.ends_with("scan.tif")),
            "{:?}",
            report.files_left
        );
        drop(_blocker);
    }

    /// Keeps `file` from being deleted until the returned value is dropped.
    #[cfg(windows)]
    fn block_removal(file: &Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        // Opened without FILE_SHARE_DELETE: nobody may delete it while it is open.
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1)
            .open(file)
            .unwrap()
    }

    /// Keeps `file` from being deleted until the returned value is dropped.
    #[cfg(unix)]
    fn block_removal(file: &Path) -> impl Drop {
        use std::os::unix::fs::PermissionsExt;
        struct Restore(PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
            }
        }
        // Removing a file needs write permission on its folder.
        let dir = file.parent().unwrap().to_path_buf();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        Restore(dir)
    }

    /// Writes the schema version into the SQLite header (byte 60), as a newer app would.
    fn set_schema_version(db: &Path, version: u32) {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = std::fs::OpenOptions::new().write(true).open(db).unwrap();
        file.seek(SeekFrom::Start(60)).unwrap();
        file.write_all(&version.to_be_bytes()).unwrap();
    }
}
