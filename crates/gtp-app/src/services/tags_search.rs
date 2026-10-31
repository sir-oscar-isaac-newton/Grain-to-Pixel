//! Tags and search over the open library (L-06). Reads use the reader connection, writes the
//! writer; every change keeps the search text in step in the same transaction (see the catalog).
//!
//! Tag names are trimmed, have inner spaces collapsed, keep their spelling, are at most 64
//! characters, and may not contain control characters. Two names are the same tag when they differ
//! only in upper and lower case (any letters, not only A to Z).

use gtp_core::{AppError, ErrorCode, FrameId, RollId, SearchQuery, Tag, TagId};

use crate::services::library::Library;
use crate::services::roll_frame::{page_offset, summary, FramePage};

pub fn tag_list(library: &Library) -> Result<Vec<Tag>, AppError> {
    library.read(|c| c.tag_list())
}

/// Adds the tags named in `add` (created when missing) to the frames and removes those named in
/// `remove` (unknown names are skipped), all or nothing.
pub fn tag_apply(
    library: &Library,
    frames: &[FrameId],
    add: &[String],
    remove: &[String],
) -> Result<(), AppError> {
    if frames.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Select at least one frame to tag.",
        ));
    }
    library.write(|c| c.tag_apply_frames(frames, add, remove))
}

/// The same for a roll: its tags count for every frame on it, also frames added later.
pub fn tag_apply_roll(
    library: &Library,
    roll: RollId,
    add: &[String],
    remove: &[String],
) -> Result<(), AppError> {
    library.write(|c| c.tag_apply_roll(roll, add, remove))
}

/// Renames a tag everywhere. The name of another tag is refused.
pub fn tag_rename(library: &Library, id: TagId, name: &str) -> Result<Tag, AppError> {
    library.write(|c| c.tag_rename(id, name))
}

/// Deletes a tag from the library, removing it from every frame and roll.
pub fn tag_delete(library: &Library, id: TagId) -> Result<(), AppError> {
    library.write(|c| c.tag_delete(id))
}

/// One page of the frames matching `query` (see [`SearchQuery`]): best text match first when there
/// is text, otherwise newest roll first. `page` counts from 0.
pub fn search(
    library: &Library,
    query: &SearchQuery,
    page: u32,
    page_size: u32,
) -> Result<FramePage, AppError> {
    let offset = page_offset(page, page_size)?;
    let (frames, total) = library.read(|c| {
        Ok((
            c.search_page(query, offset, page_size)?,
            c.search_count(query)?,
        ))
    })?;
    let items = frames.into_iter().map(|f| summary(library, f)).collect();
    Ok(FramePage {
        items,
        total,
        page,
        page_size,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gtp_core::{Frame, FrameKind};

    use super::*;
    use crate::library_fs::thumb_rel;
    use crate::services::roll_frame::{roll_create, RollInput, MAX_PAGE_SIZE};

    struct TempLib {
        dir: PathBuf,
        lib: Library,
    }

    impl TempLib {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "gtp-tagssearch-{label}-{}-{}",
                std::process::id(),
                FrameId::new()
            ));
            let lib = Library::create(&dir, "Test").unwrap();
            Self { dir, lib }
        }

        fn roll(&self, name: &str, camera: &str) -> RollId {
            roll_create(
                &self.lib,
                RollInput {
                    name: name.into(),
                    camera: Some(camera.into()),
                    film_stock: Some("Kodak Portra 400".into()),
                    shot_from: Some("2026-07".into()),
                    ..RollInput::default()
                },
            )
            .unwrap()
            .id
        }

        fn frame(&self, roll: RollId, index: u32) -> FrameId {
            let frame = Frame {
                id: FrameId::new(),
                roll_id: roll,
                index_in_roll: index,
                kind: FrameKind::ScanRgbIr,
                width: 10,
                height: 10,
                has_ir: false,
                note: String::new(),
                created_at: String::new(),
                edited_at: None,
            };
            self.lib.write(|c| c.insert_frame(&frame)).unwrap().id
        }
    }

    impl Drop for TempLib {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn tags_are_applied_by_name_found_by_search_and_managed() {
        let t = TempLib::new("tags");
        let roll = t.roll("Japan", "Nikon F3");
        let (f1, f2) = (t.frame(roll, 1), t.frame(roll, 2));
        let other = t.roll("Home", "Leica M6");
        let f3 = t.frame(other, 1);

        tag_apply(
            &t.lib,
            &[f1, f2],
            &names(&["Night Market", " street "]),
            &[],
        )
        .unwrap();
        tag_apply_roll(&t.lib, roll, &names(&["Japan Trip"]), &[]).unwrap();
        let list: Vec<String> = tag_list(&t.lib)
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(list, ["Japan Trip", "Night Market", "street"]);

        let street = tag_list(&t.lib)
            .unwrap()
            .into_iter()
            .find(|t| t.name == "street")
            .unwrap();
        let by_tag = |tags: Vec<TagId>| {
            search(
                &t.lib,
                &SearchQuery {
                    tags_all: tags,
                    ..SearchQuery::default()
                },
                0,
                50,
            )
            .unwrap()
        };
        assert_eq!(by_tag(vec![street.id]).total, 2);
        let trip = tag_list(&t.lib)
            .unwrap()
            .into_iter()
            .find(|t| t.name == "Japan Trip")
            .unwrap();
        assert_eq!(
            by_tag(vec![trip.id]).total,
            2,
            "inherited by the roll's frames"
        );
        assert!(by_tag(vec![trip.id])
            .items
            .iter()
            .all(|i| i.roll_id == roll));

        // Rename shows in search text; a clash is refused; delete removes it everywhere.
        let renamed = tag_rename(&t.lib, street.id, "Street Photography").unwrap();
        assert_eq!(renamed.name, "Street Photography");
        let hits = |text: &str| {
            search(
                &t.lib,
                &SearchQuery {
                    text: Some(text.into()),
                    ..SearchQuery::default()
                },
                0,
                50,
            )
            .unwrap()
            .total
        };
        assert_eq!(hits("photography"), 2);
        let clash = tag_rename(&t.lib, street.id, "NIGHT market").unwrap_err();
        assert_eq!(clash.code, ErrorCode::InvalidInput);
        tag_delete(&t.lib, street.id).unwrap();
        assert_eq!(hits("photography"), 0);
        assert_eq!(
            tag_delete(&t.lib, street.id).unwrap_err().code,
            ErrorCode::NotFound
        );

        // The other roll is untouched by all of it.
        assert_eq!(hits("leica"), 1);
        let _ = f3;
    }

    #[test]
    fn tag_rules_are_enforced_at_the_service() {
        let t = TempLib::new("rules");
        let f = t.frame(t.roll("R", "Nikon"), 1);
        let long = "x".repeat(65);
        for bad in ["", "   ", "bell\u{7}", long.as_str()] {
            let err = tag_apply(&t.lib, &[f], &names(&[bad]), &[]).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{bad:?}");
        }
        assert!(tag_list(&t.lib).unwrap().is_empty(), "nothing was created");
        // Duplicates and spellings of one tag make one tag.
        tag_apply(&t.lib, &[f], &names(&["Été", "ÉTÉ", " été "]), &[]).unwrap();
        assert_eq!(tag_list(&t.lib).unwrap().len(), 1);
        let none = tag_apply(&t.lib, &[], &names(&["x"]), &[]).unwrap_err();
        assert_eq!(none.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn search_results_are_pages_with_totals_and_thumbnail_flags() {
        let t = TempLib::new("pages");
        let roll = t.roll("Pages", "Nikon F3");
        let frames: Vec<FrameId> = (1..=5).map(|i| t.frame(roll, i)).collect();
        std::fs::write(t.lib.resolve(&thumb_rel(frames[0])).unwrap(), b"jpg").unwrap();
        let q = SearchQuery {
            camera: Some("nikon f3".into()),
            date_from: Some("2026-07-31".into()),
            ..SearchQuery::default()
        };
        let first = search(&t.lib, &q, 0, 2).unwrap();
        assert_eq!(
            (first.total, first.items.len(), first.page, first.page_size),
            (5, 2, 0, 2)
        );
        assert_eq!(first.items[0].id, frames[0]);
        assert!(first.items[0].has_thumb && !first.items[1].has_thumb);
        let last = search(&t.lib, &q, 2, 2).unwrap();
        assert_eq!(last.items.len(), 1);
        assert_eq!(last.items[0].id, frames[4]);
        assert!(search(&t.lib, &q, 3, 2).unwrap().items.is_empty());
    }

    #[test]
    fn bad_search_input_is_rejected_with_invalid_input() {
        let t = TempLib::new("bad");
        let ok = SearchQuery::default();
        for (page, size) in [(0, 0), (0, MAX_PAGE_SIZE + 1), (u32::MAX, 2)] {
            let err = search(&t.lib, &ok, page, size).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{page}/{size}");
        }
        let cases = [
            SearchQuery {
                date_from: Some("2026-8".into()),
                ..ok.clone()
            },
            SearchQuery {
                date_from: Some("2026-09".into()),
                date_to: Some("2026-08".into()),
                ..ok.clone()
            },
            SearchQuery {
                iso: Some(0),
                ..ok.clone()
            },
            SearchQuery {
                text: Some("w".repeat(201)),
                ..ok.clone()
            },
        ];
        for q in cases {
            assert_eq!(
                search(&t.lib, &q, 0, 10).unwrap_err().code,
                ErrorCode::InvalidInput
            );
        }
    }

    #[test]
    fn a_read_only_library_can_be_searched_but_not_tagged() {
        use std::io::{Seek, SeekFrom, Write};
        let dir = std::env::temp_dir().join(format!(
            "gtp-tagssearch-readonly-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let library = Library::create(&dir, "Test").unwrap();
        let roll = roll_create(
            &library,
            RollInput {
                name: "R".into(),
                ..RollInput::default()
            },
        )
        .unwrap()
        .id;
        let frame = Frame {
            id: FrameId::new(),
            roll_id: roll,
            index_in_roll: 1,
            kind: FrameKind::ScanRgbIr,
            width: 10,
            height: 10,
            has_ir: false,
            note: String::new(),
            created_at: String::new(),
            edited_at: None,
        };
        let f = library.write(|c| c.insert_frame(&frame)).unwrap().id;
        drop(library);
        let db = dir.join(crate::library_fs::DB_FILE);
        // The library was made by a newer app version: its schema number is higher than ours.
        {
            let mut file = std::fs::OpenOptions::new().write(true).open(&db).unwrap();
            file.seek(SeekFrom::Start(60)).unwrap();
            file.write_all(&(gtp_catalog::SCHEMA_VERSION + 1).to_be_bytes())
                .unwrap();
        }
        let readonly = Library::open(&dir).unwrap();
        assert!(readonly.is_read_only());
        assert_eq!(
            search(&readonly, &SearchQuery::default(), 0, 10)
                .unwrap()
                .total,
            1
        );
        let err = tag_apply(&readonly, &[f], &names(&["x"]), &[]).unwrap_err();
        assert_eq!(err.code, ErrorCode::LibraryTooNew);
        drop(readonly);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
