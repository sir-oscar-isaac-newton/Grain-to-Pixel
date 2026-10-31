//! The catalog against real SQLite files in a temporary folder.

// Test code (convention: tests may unwrap); clippy.toml only exempts `#[test]` functions, not the
// helpers of an integration-test file.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use gtp_catalog::{backup_path, Catalog, SCHEMA_VERSION};
use gtp_core::{
    ErrorCode, FilmFormat, Frame, FrameId, FrameKind, Recipe, Roll, RollId, SourceFile, SourceRole,
};
use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gtp-catalog-{label}-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn db(&self) -> PathBuf {
        self.0.join("library.db")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn user_version(path: &Path) -> u32 {
    Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn count(cat_path: &Path, table: &str) -> i64 {
    Connection::open(cat_path)
        .unwrap()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn roll(name: &str) -> Roll {
    Roll {
        id: RollId::new(),
        name: name.into(),
        film_stock: Some("Kodak Portra 400".into()),
        format: FilmFormat::Mm35,
        camera: Some("Nikon F3".into()),
        iso_box: Some(400),
        iso_shot: Some(800),
        shot_from: Some("2026-07".into()),
        shot_to: Some("2026-08-02".into()),
        notes: "  pushed one stop ".into(),
        film_base_rgb: None,
        film_base_confidence: None,
        created_at: String::new(),
    }
}

fn frame(roll: RollId, index: u32) -> Frame {
    Frame {
        id: FrameId::new(),
        roll_id: roll,
        index_in_roll: index,
        kind: FrameKind::ScanRgbIr,
        width: 5616,
        height: 3744,
        has_ir: true,
        note: String::new(),
        created_at: String::new(),
        edited_at: None,
    }
}

#[test]
fn an_empty_library_is_migrated_and_reopening_changes_nothing() {
    let dir = TempDir::new("fresh");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let stored = cat.insert_roll(&roll("Summer")).unwrap();
    assert_eq!(stored.notes, "pushed one stop", "texts are trimmed");
    assert!(stored.created_at.ends_with('Z') && stored.created_at.len() == 20);
    cat.close().unwrap();
    assert_eq!(user_version(&dir.db()), SCHEMA_VERSION);

    let cat = Catalog::open(&dir.db()).unwrap();
    assert_eq!(cat.list_rolls().unwrap(), vec![stored]);
    assert!(!cat.recovered_from_unclean_exit());
    assert_eq!(user_version(&dir.db()), SCHEMA_VERSION);
    assert!(!backup_path(&dir.db(), SCHEMA_VERSION).exists());
    assert!(
        !backup_path(&dir.db(), 0).exists(),
        "a new file needs no backup"
    );
    cat.integrity_check().unwrap();
}

#[test]
fn an_older_library_is_backed_up_before_it_is_migrated() {
    let dir = TempDir::new("backup");
    {
        let conn = Connection::open(dir.db()).unwrap();
        conn.execute_batch(include_str!("../migrations/0001_init.sql"))
            .unwrap();
        conn.execute(
            "INSERT INTO roll(id, name, created_at) VALUES (?1, 'Old roll', '2026-01-01T00:00:00Z')",
            [RollId::new().to_string()],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
    }
    let cat = Catalog::open(&dir.db()).unwrap();
    assert_eq!(cat.list_rolls().unwrap().len(), 1);
    drop(cat);

    let backup = backup_path(&dir.db(), 1);
    assert!(backup.exists());
    assert_eq!(user_version(&backup), 1, "the backup is the old schema");
    assert_eq!(count(&backup, "roll"), 1);
    assert_eq!(user_version(&dir.db()), SCHEMA_VERSION);
}

#[test]
fn a_v3_library_upgrades_to_v4_with_a_backup_and_keeps_its_rolls() {
    let dir = TempDir::new("upgrade-v3");
    let id = RollId::new();
    {
        let conn = Connection::open(dir.db()).unwrap();
        for sql in [
            include_str!("../migrations/0001_init.sql"),
            include_str!("../migrations/0002_search.sql"),
            include_str!("../migrations/0003_export_log.sql"),
        ] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute(
            "INSERT INTO roll(id, name, film_base_r, film_base_g, film_base_b, created_at)
             VALUES (?1, 'Old', 0.8, 0.5, 0.3, '2026-01-01T00:00:00Z')",
            [id.to_string()],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 3).unwrap();
    }
    let cat = Catalog::open(&dir.db()).unwrap();
    let old = cat.get_roll(id).unwrap();
    assert_eq!(old.film_base_rgb, Some([0.8, 0.5, 0.3]));
    assert_eq!(
        old.film_base_confidence, None,
        "an existing base has no confidence"
    );
    assert!(!old.film_base_uncertain());
    drop(cat);

    assert_eq!(SCHEMA_VERSION, 4);
    assert_eq!(user_version(&dir.db()), 4);
    let backup = backup_path(&dir.db(), 3);
    assert!(backup.exists());
    assert_eq!(user_version(&backup), 3, "the backup is the old schema");
    let backed_up = Connection::open(&backup).unwrap();
    let has_column = |conn: &Connection| {
        conn.prepare("SELECT film_base_confidence FROM roll")
            .is_ok()
    };
    assert!(!has_column(&backed_up));
    assert!(has_column(&Connection::open(dir.db()).unwrap()));
}

#[test]
fn the_roll_base_is_stored_only_when_unset_and_checked() {
    let dir = TempDir::new("base-if-unset");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("R")).unwrap();
    let (stored, set) = cat
        .set_roll_film_base_if_unset(r.id, [0.8, 0.5, 0.3], Some(0.7))
        .unwrap();
    assert!(set);
    assert_eq!(stored.film_base_confidence, Some(0.7));
    let (again, set) = cat
        .set_roll_film_base_if_unset(r.id, [0.1, 0.1, 0.1], Some(0.99))
        .unwrap();
    assert!(!set);
    assert_eq!(again.film_base_rgb, Some([0.8, 0.5, 0.3]));
    for bad in [
        ([0.0, 0.5, 0.3], Some(0.5)),
        ([0.8, 0.5, 0.3], Some(1.2)),
        ([0.8, 0.5, 0.3], Some(f32::NAN)),
    ] {
        let err = cat
            .set_roll_film_base_if_unset(r.id, bad.0, bad.1)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput, "{bad:?}");
    }
    let err = cat
        .set_roll_film_base_if_unset(RollId::new(), [0.8, 0.5, 0.3], None)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    // A confidence without a base is refused when a roll is saved.
    let mut orphan = cat.get_roll(r.id).unwrap();
    orphan.film_base_rgb = None;
    orphan.film_base_confidence = Some(0.5);
    assert_eq!(
        cat.update_roll(&orphan).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn a_newer_library_is_refused_and_left_alone() {
    let dir = TempDir::new("newer");
    {
        let conn = Connection::open(dir.db()).unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 7)
            .unwrap();
    }
    let err = Catalog::open(&dir.db()).err().unwrap();
    assert_eq!(err.code, ErrorCode::LibraryTooNew);
    assert_eq!(user_version(&dir.db()), SCHEMA_VERSION + 7);
}

#[test]
fn a_reader_sees_committed_writes_and_cannot_write() {
    let dir = TempDir::new("reader");
    let mut writer = Catalog::open(&dir.db()).unwrap();
    let reader = Catalog::open_reader(&dir.db()).unwrap();
    assert!(writer.is_writer() && !reader.is_writer());
    assert!(reader.list_rolls().unwrap().is_empty());
    let r = writer.insert_roll(&roll("Seen")).unwrap();
    assert_eq!(reader.list_rolls().unwrap(), vec![r]);
    assert_eq!(reader.schema_version().unwrap(), SCHEMA_VERSION);

    let mut reader = reader;
    assert_eq!(
        reader.insert_roll(&roll("Nope")).unwrap_err().code,
        ErrorCode::Db
    );
    drop(reader);
    drop(writer);
    assert!(!Catalog::open(&dir.db())
        .unwrap()
        .recovered_from_unclean_exit());
}

#[test]
fn a_newer_library_can_still_be_read() {
    let dir = TempDir::new("newer-read");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("Future")).unwrap();
    cat.close().unwrap();
    Connection::open(dir.db())
        .unwrap()
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    let reader = Catalog::open_reader(&dir.db()).unwrap();
    assert_eq!(reader.schema_version().unwrap(), SCHEMA_VERSION + 1);
    assert_eq!(reader.list_rolls().unwrap(), vec![r]);
    let missing = Catalog::open_reader(&dir.0.join("nope.db")).err().unwrap();
    assert_eq!(missing.code, ErrorCode::NotFound);
}

#[test]
fn an_unclean_exit_is_noticed_and_checked_on_the_next_open() {
    let dir = TempDir::new("unclean");
    let cat = Catalog::open(&dir.db()).unwrap();
    // A crash: the catalog is never closed or dropped.
    std::mem::forget(cat);
    let cat = Catalog::open(&dir.db()).unwrap();
    assert!(cat.recovered_from_unclean_exit());
    cat.close().unwrap();
    assert!(!Catalog::open(&dir.db())
        .unwrap()
        .recovered_from_unclean_exit());
}

#[test]
fn deleting_a_roll_cascades_to_everything_below_it() {
    let dir = TempDir::new("cascade");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("Trip")).unwrap();
    let keep = cat.insert_roll(&roll("Other")).unwrap();
    let f1 = cat.insert_frame(&frame(r.id, 1)).unwrap();
    let f2 = cat.insert_frame(&frame(r.id, 2)).unwrap();
    let other = cat.insert_frame(&frame(keep.id, 1)).unwrap();
    cat.insert_source(&SourceFile {
        frame_id: f1.id,
        role: SourceRole::Rgb,
        rel_path: "originals/a/b/scan.tif".into(),
        sha256: "00".repeat(32),
        bytes: 123_456_789,
    })
    .unwrap();
    cat.save_recipe(f1.id, &Recipe::default()).unwrap();
    let tag = cat.tag_ensure("Japan").unwrap();
    cat.tag_frames(&[f2.id], &[tag.id], &[]).unwrap();
    cat.tag_roll(r.id, &[tag.id], &[]).unwrap();

    cat.delete_roll(r.id).unwrap();
    assert_eq!(cat.get_roll(r.id).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(cat.get_frame(f1.id).unwrap_err().code, ErrorCode::NotFound);
    assert!(cat.sources_of(f1.id).unwrap().is_empty());
    assert!(cat.load_recipe(f1.id).unwrap().is_none());
    drop(cat);
    for table in ["frame_tag", "roll_tag", "recipe", "source_file"] {
        assert_eq!(count(&dir.db(), table), 0, "{table}");
    }
    assert_eq!(count(&dir.db(), "frame"), 1);
    assert_eq!(
        count(&dir.db(), "search"),
        1,
        "only the other roll's frame is left"
    );
    assert_eq!(count(&dir.db(), "tag"), 1, "the tag itself stays");

    let mut cat = Catalog::open(&dir.db()).unwrap();
    assert_eq!(cat.delete_frames(&[other.id, FrameId::new()]).unwrap(), 1);
    assert_eq!(cat.delete_roll(r.id).unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn tag_names_are_unique_across_unicode_case_and_keep_their_spelling() {
    let dir = TempDir::new("tags");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let ete = cat.tag_ensure("  Été  à Paris ").unwrap();
    assert_eq!(ete.name, "Été à Paris");
    assert_eq!(cat.tag_ensure("ÉTÉ À PARIS").unwrap(), ete);
    let munich = cat.tag_ensure("München").unwrap();
    assert_eq!(cat.tag_ensure("MÜNCHEN").unwrap().id, munich.id);
    assert_ne!(
        cat.tag_ensure("Ete a Paris").unwrap().id,
        ete.id,
        "accents count"
    );

    let err = cat.tag_rename(munich.id, "été à paris").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    assert_eq!(
        cat.tag_rename(munich.id, "MÜNCHEN").unwrap().name,
        "MÜNCHEN"
    );
    let names: Vec<String> = cat
        .tag_list()
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, ["Ete a Paris", "MÜNCHEN", "Été à Paris"]);
}

#[test]
fn search_covers_roll_fields_notes_and_own_and_roll_tags() {
    let dir = TempDir::new("search");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("Summer")).unwrap();
    let mut plain = roll("Winter");
    plain.camera = Some("Leica M6".into());
    plain.film_stock = Some("Ilford HP5".into());
    let w = cat.insert_roll(&plain).unwrap();
    let f1 = cat.insert_frame(&frame(r.id, 1)).unwrap();
    let wf = cat.insert_frame(&frame(w.id, 1)).unwrap();

    assert_eq!(cat.search_frames("nikon f3").unwrap(), vec![f1.id]);
    assert_eq!(cat.search_frames("portra").unwrap(), vec![f1.id]);
    assert_eq!(cat.search_frames("hp5").unwrap(), vec![wf.id]);
    assert_eq!(cat.search_frames("2026-07").unwrap().len(), 2);
    assert!(cat.search_frames("").unwrap().is_empty());
    assert!(cat.search_frames("\"unbalanced OR").unwrap().is_empty());

    cat.set_frame_note(f1.id, "Lighthouse at dusk").unwrap();
    assert_eq!(cat.search_frames("lighth").unwrap(), vec![f1.id]);

    // A roll tag reaches frames added later too; search ignores accents.
    let trip = cat.tag_ensure("Été").unwrap();
    cat.tag_roll(r.id, &[trip.id], &[]).unwrap();
    let f2 = cat.insert_frame(&frame(r.id, 2)).unwrap();
    let mut found = cat.search_frames("ete").unwrap();
    found.sort();
    let mut want = vec![f1.id, f2.id];
    want.sort();
    assert_eq!(found, want);
    assert_eq!(cat.frame_tags(f2.id).unwrap().from_roll, vec![trip.clone()]);

    // A frame keeps a tag it has itself when the roll loses it.
    cat.tag_frames(&[f2.id], &[trip.id], &[]).unwrap();
    cat.tag_roll(r.id, &[], &[trip.id]).unwrap();
    assert_eq!(cat.search_frames("ete").unwrap(), vec![f2.id]);
    assert_eq!(cat.frame_tags(f2.id).unwrap().own, vec![trip.clone()]);

    cat.tag_rename(trip.id, "Sommer").unwrap();
    assert!(cat.search_frames("ete").unwrap().is_empty());
    assert_eq!(cat.search_frames("sommer").unwrap(), vec![f2.id]);
    cat.tag_delete(trip.id).unwrap();
    assert!(cat.search_frames("sommer").unwrap().is_empty());

    let mut renamed = cat.get_roll(r.id).unwrap();
    renamed.camera = Some("Olympus OM-1".into());
    cat.update_roll(&renamed).unwrap();
    assert!(cat.search_frames("nikon").unwrap().is_empty());
    assert_eq!(cat.search_frames("olympus").unwrap().len(), 2);

    cat.delete_frames(&[f1.id]).unwrap();
    assert_eq!(cat.search_frames("olympus").unwrap(), vec![f2.id]);
}

#[test]
fn a_failed_change_leaves_tags_and_search_untouched() {
    let dir = TempDir::new("rollback");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("Summer")).unwrap();
    let f1 = cat.insert_frame(&frame(r.id, 1)).unwrap();
    let tag = cat.tag_ensure("keeper").unwrap();
    // The second frame does not exist, so the whole change is rolled back.
    let err = cat
        .tag_frames(&[f1.id, FrameId::new()], &[tag.id], &[])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(cat.frame_tags(f1.id).unwrap().own.is_empty());
    assert!(cat.search_frames("keeper").unwrap().is_empty());
}

#[test]
fn recipes_round_trip_and_stamp_the_frame() {
    let dir = TempDir::new("recipe");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("Summer")).unwrap();
    let f = cat.insert_frame(&frame(r.id, 1)).unwrap();
    assert!(f.edited_at.is_none());
    assert!(cat.load_recipe(f.id).unwrap().is_none());
    let mut recipe = Recipe::default();
    recipe.tone.exposure_ev = 0.7;
    cat.save_recipe(f.id, &recipe).unwrap();
    cat.save_recipe(f.id, &recipe).unwrap();
    assert_eq!(cat.load_recipe(f.id).unwrap(), Some(recipe.clone()));
    assert!(cat.get_frame(f.id).unwrap().edited_at.is_some());
    let err = cat.save_recipe(FrameId::new(), &recipe).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn an_imported_frame_is_numbered_and_stored_with_its_sources_and_recipe() {
    let dir = TempDir::new("import");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("Scans")).unwrap();
    let mut recipe = Recipe::default();
    recipe.tone.exposure_ev = 0.3;
    let mut stored = Vec::new();
    for _ in 0..2 {
        let f = frame(r.id, 99);
        let sources = [SourceRole::Rgb, SourceRole::Ir].map(|role| SourceFile {
            frame_id: f.id,
            role,
            rel_path: format!("originals/{}/{}/{role:?}.tif", r.id, f.id),
            sha256: "ab".repeat(32),
            bytes: 42,
        });
        stored.push(cat.insert_imported_frame(&f, &sources, &recipe).unwrap());
    }
    assert_eq!(
        stored[0].index_in_roll, 1,
        "numbering starts at 1, the given index is ignored"
    );
    assert_eq!(stored[1].index_in_roll, 2);
    assert!(stored[0].edited_at.is_none(), "an import is not an edit");
    assert_eq!(cat.sources_of(stored[0].id).unwrap().len(), 2);
    assert_eq!(cat.load_recipe(stored[1].id).unwrap(), Some(recipe.clone()));
    assert_eq!(cat.search_frames("portra").unwrap().len(), 2);

    // A source of another frame fails the whole insert: nothing is left behind.
    let f = frame(r.id, 1);
    let wrong = SourceFile {
        frame_id: FrameId::new(),
        role: SourceRole::Rgb,
        rel_path: "x".into(),
        sha256: String::new(),
        bytes: 0,
    };
    assert!(cat.insert_imported_frame(&f, &[wrong], &recipe).is_err());
    assert_eq!(cat.get_frame(f.id).unwrap_err().code, ErrorCode::NotFound);
    let orphan = frame(RollId::new(), 1);
    assert_eq!(
        cat.insert_imported_frame(&orphan, &[], &recipe)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(cat.count_frames(None).unwrap(), 2);
}

#[test]
fn invalid_roll_and_frame_input_is_rejected() {
    let dir = TempDir::new("invalid");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let cases = [
        Roll {
            name: "   ".into(),
            ..roll("x")
        },
        Roll {
            iso_shot: Some(0),
            ..roll("x")
        },
        Roll {
            shot_from: Some("2026-08".into()),
            shot_to: Some("2026-07".into()),
            ..roll("x")
        },
        Roll {
            camera: Some("c".repeat(201)),
            ..roll("x")
        },
    ];
    for bad in cases {
        assert_eq!(
            cat.insert_roll(&bad).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
    let orphan = cat.insert_frame(&frame(RollId::new(), 1)).unwrap_err();
    assert_eq!(orphan.code, ErrorCode::InvalidInput);
    assert!(cat.list_rolls().unwrap().is_empty());
}

#[test]
fn rolls_and_frames_list_in_order_and_meta_round_trips() {
    let dir = TempDir::new("order");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let a = cat.insert_roll(&roll("A")).unwrap();
    let b = cat.insert_roll(&roll("B")).unwrap();
    let ids: Vec<RollId> = cat.list_rolls().unwrap().iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![b.id, a.id], "newest first");
    let f3 = cat.insert_frame(&frame(a.id, 3)).unwrap();
    let f1 = cat.insert_frame(&frame(a.id, 1)).unwrap();
    let order: Vec<FrameId> = cat
        .list_frames(a.id)
        .unwrap()
        .iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(order, vec![f1.id, f3.id]);

    let fb = cat.insert_frame(&frame(b.id, 1)).unwrap();
    let page = |roll, offset, limit| -> Vec<FrameId> {
        cat.list_frames_page(roll, offset, limit)
            .unwrap()
            .iter()
            .map(|f| f.id)
            .collect()
    };
    assert_eq!(
        page(None, 0, 10),
        vec![fb.id, f1.id, f3.id],
        "newest roll first"
    );
    assert_eq!(page(None, 1, 1), vec![f1.id]);
    assert_eq!(page(Some(a.id), 0, 10), vec![f1.id, f3.id]);
    assert!(page(Some(a.id), 5, 10).is_empty());
    assert_eq!(cat.count_frames(None).unwrap(), 3);
    assert_eq!(cat.count_frames(Some(a.id)).unwrap(), 2);
    assert_eq!(cat.count_frames(Some(RollId::new())).unwrap(), 0);

    assert_eq!(cat.meta_get("library_name").unwrap(), None);
    cat.meta_set("library_name", "Negatives").unwrap();
    cat.meta_set("library_name", "Film").unwrap();
    assert_eq!(
        cat.meta_get("library_name").unwrap().as_deref(),
        Some("Film")
    );

    let mut based = cat.get_roll(a.id).unwrap();
    based.film_base_rgb = Some([0.8, 0.55, 0.35]);
    assert_eq!(
        cat.update_roll(&based).unwrap().film_base_rgb,
        Some([0.8, 0.55, 0.35])
    );
}
