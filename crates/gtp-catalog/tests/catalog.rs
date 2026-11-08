//! The catalog against real SQLite files in a temporary folder.

// Test code (convention: tests may unwrap); clippy.toml only exempts `#[test]` functions, not the
// helpers of an integration-test file.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use gtp_catalog::{backup_path, Catalog, SCHEMA_VERSION};
use gtp_core::{
    BaseChange, ErrorCode, FilmFormat, Frame, FrameId, FrameKind, FrameSize, Recipe, Roll, RollId,
    SearchQuery, SourceFile, SourceRole,
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
        frame_size: None,
        has_flat_field: false,
        color_balance: None,
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
fn a_v3_library_upgrades_with_a_backup_and_keeps_its_rolls() {
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
    assert_eq!(old.frame_size, None, "an existing roll has no frame size");
    assert_eq!(old.format, FilmFormat::Mm35);
    drop(cat);

    assert_eq!(SCHEMA_VERSION, 9);
    assert_eq!(user_version(&dir.db()), 9);
    assert!(!old.has_flat_field, "an existing roll has no flat field");
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
fn a_v7_library_upgrades_to_v8_and_its_rolls_have_no_colour_balance() {
    let dir = TempDir::new("upgrade-v7");
    let id = RollId::new();
    {
        let conn = Connection::open(dir.db()).unwrap();
        for sql in [
            include_str!("../migrations/0001_init.sql"),
            include_str!("../migrations/0002_search.sql"),
            include_str!("../migrations/0003_export_log.sql"),
            include_str!("../migrations/0004_roll_base_confidence.sql"),
            include_str!("../migrations/0005_roll_frame_size.sql"),
            include_str!("../migrations/0006_frame_scan.sql"),
            include_str!("../migrations/0007_flat_field.sql"),
        ] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute(
            "INSERT INTO roll(id, name, film_base_r, film_base_g, film_base_b, created_at)
             VALUES (?1, 'Old', 0.8, 0.5, 0.3, '2026-01-01T00:00:00Z')",
            [id.to_string()],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 7).unwrap();
    }
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let old = cat.get_roll(id).unwrap();
    assert_eq!(old.film_base_rgb, Some([0.8, 0.5, 0.3]));
    assert_eq!(old.color_balance, None);
    assert!(cat.store_roll_color_balance(id, [1.0, 2.0, 3.0]).unwrap().1);
    drop(cat);

    assert_eq!(user_version(&dir.db()), 9);
    let backup = backup_path(&dir.db(), 7);
    assert_eq!(user_version(&backup), 7, "the backup is the old schema");
    assert!(Connection::open(&backup)
        .unwrap()
        .prepare("SELECT balance_red_cyan FROM roll")
        .is_err());
}

#[test]
fn a_rolls_colour_balance_is_stored_once_and_survives_editing_the_roll() {
    let dir = TempDir::new("colour-balance");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("R")).unwrap();
    assert_eq!(r.color_balance, None);

    let (stored, changed) = cat
        .store_roll_color_balance(r.id, [4.0, -2.5, 7.0])
        .unwrap();
    assert!(changed);
    assert_eq!(stored.color_balance, Some([4.0, -2.5, 7.0]));

    // The first one stays.
    let (kept, changed) = cat.store_roll_color_balance(r.id, [9.0, 9.0, 9.0]).unwrap();
    assert!(!changed);
    assert_eq!(kept.color_balance, Some([4.0, -2.5, 7.0]));

    // Editing the roll's details, even with a stale copy of the roll, leaves it alone.
    let mut edited = roll("Renamed");
    edited.id = r.id;
    edited.color_balance = None;
    let updated = cat.update_roll(&edited).unwrap();
    assert_eq!(updated.name, "Renamed");
    assert_eq!(updated.color_balance, Some([4.0, -2.5, 7.0]));
    assert_eq!(
        cat.list_rolls().unwrap()[0].color_balance,
        Some([4.0, -2.5, 7.0])
    );

    // Values that are not numbers or beyond the sliders are refused, an unknown roll is not found.
    let other = cat.insert_roll(&roll("Other")).unwrap();
    for bad in [[f32::NAN, 0.0, 0.0], [0.0, 101.0, 0.0]] {
        assert_eq!(
            cat.store_roll_color_balance(other.id, bad)
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }
    assert_eq!(cat.get_roll(other.id).unwrap().color_balance, None);
    assert_eq!(
        cat.store_roll_color_balance(RollId::new(), [0.0; 3])
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[test]
fn the_colour_axes_are_written_only_into_recipes_that_were_never_edited() {
    let dir = TempDir::new("colour-axes");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("R")).unwrap();
    let mut recipe = Recipe::default();
    recipe.tone.exposure_ev = 0.5;
    let untouched = cat
        .insert_imported_frame(&frame(r.id, 1), &[], &recipe)
        .unwrap();
    let edited = cat
        .insert_imported_frame(&frame(r.id, 2), &[], &recipe)
        .unwrap();
    cat.save_recipe(edited.id, &recipe).unwrap();
    assert!(cat.get_frame(edited.id).unwrap().edited_at.is_some());

    let changed = cat
        .set_unedited_color_axes(&[untouched.id, edited.id], [3.0, -1.0, 2.0])
        .unwrap();
    assert_eq!(changed, vec![untouched.id]);
    let after = cat.load_recipe(untouched.id).unwrap().unwrap();
    assert_eq!(
        [
            after.color.red_cyan,
            after.color.green_magenta,
            after.color.blue_yellow
        ],
        [3.0, -1.0, 2.0]
    );
    assert_eq!(
        after.tone.exposure_ev, 0.5,
        "the rest of the recipe is kept"
    );
    assert!(
        cat.get_frame(untouched.id).unwrap().edited_at.is_none(),
        "the automatic recipe changed, the frame is not edited"
    );
    let theirs = cat.load_recipe(edited.id).unwrap().unwrap();
    assert_eq!(theirs.color.red_cyan, 0.0, "an edited frame is left alone");

    // The same axes again change nothing.
    assert!(cat
        .set_unedited_color_axes(&[untouched.id], [3.0, -1.0, 2.0])
        .unwrap()
        .is_empty());
}

#[test]
fn the_format_and_frame_size_of_a_roll_are_stored_and_measurements_average_in() {
    let dir = TempDir::new("frame-size");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let mut input = roll("R");
    input.format = FilmFormat::Mf645;
    let r = cat.insert_roll(&input).unwrap();
    assert_eq!(r.format, FilmFormat::Mf645);
    assert_eq!(r.frame_size, None);

    let one = cat.record_frame_measurement(r.id, [0.5, 0.8]).unwrap();
    assert_eq!(
        one.frame_size,
        Some(FrameSize {
            w: 0.5,
            h: 0.8,
            samples: 1
        })
    );
    let two = cat.record_frame_measurement(r.id, [0.52, 0.78]).unwrap();
    let size = two.frame_size.unwrap();
    assert_eq!(size.samples, 2);
    assert!((size.w - 0.51).abs() < 1e-6 && (size.h - 0.79).abs() < 1e-6);

    // Set by hand (0 samples), it stays; the format and the size survive an update.
    let mut by_hand = cat.get_roll(r.id).unwrap();
    by_hand.format = FilmFormat::Mf6x7;
    by_hand.frame_size = Some(FrameSize {
        w: 0.4,
        h: 0.9,
        samples: 0,
    });
    cat.update_roll(&by_hand).unwrap();
    let kept = cat.record_frame_measurement(r.id, [0.1, 0.1]).unwrap();
    assert_eq!(kept.frame_size, by_hand.frame_size);
    assert_eq!(kept.format, FilmFormat::Mf6x7);

    for bad in [[0.0, 0.5], [0.5, 1.5], [f32::NAN, 0.5]] {
        assert_eq!(
            cat.record_frame_measurement(r.id, bad).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
    assert_eq!(
        cat.record_frame_measurement(RollId::new(), [0.5, 0.5])
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    by_hand.frame_size = Some(FrameSize {
        w: 2.0,
        h: 0.5,
        samples: 0,
    });
    assert_eq!(
        cat.update_roll(&by_hand).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn a_rolls_flat_field_is_stored_replaced_cleared_and_removed_with_the_roll() {
    use gtp_core::FlatField;
    let dir = TempDir::new("flat-field");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("R")).unwrap();
    let other = cat.insert_roll(&roll("Other")).unwrap();
    assert_eq!(cat.get_flat_field(r.id).unwrap(), None);
    assert!(!cat.get_roll(r.id).unwrap().has_flat_field);

    let flat = |value: f32| FlatField {
        grid_w: 3,
        grid_h: 2,
        frame_w: 600,
        frame_h: 400,
        light: (0..18).map(|i| value + i as f32 * 0.01).collect(),
        sources: vec!["r.NEF".into(), "g.NEF".into(), "b.NEF".into()],
    };
    cat.set_flat_field(r.id, &flat(0.5)).unwrap();
    assert_eq!(cat.get_flat_field(r.id).unwrap(), Some(flat(0.5)));
    assert!(cat.get_roll(r.id).unwrap().has_flat_field);
    assert!(
        !cat.get_roll(other.id).unwrap().has_flat_field,
        "only that roll"
    );
    assert!(cat
        .list_rolls()
        .unwrap()
        .iter()
        .any(|x| x.id == r.id && x.has_flat_field));

    // A new one replaces the old.
    cat.set_flat_field(r.id, &flat(0.7)).unwrap();
    assert_eq!(cat.get_flat_field(r.id).unwrap(), Some(flat(0.7)));

    // A grid that is not usable, and a roll that is not there, are refused.
    let mut bad = flat(0.5);
    bad.light.pop();
    assert_eq!(
        cat.set_flat_field(r.id, &bad).unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        cat.set_flat_field(RollId::new(), &flat(0.5))
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        cat.get_flat_field(r.id).unwrap(),
        Some(flat(0.7)),
        "unchanged by the refusals"
    );

    // Clearing says whether there was one; deleting the roll takes its flat field with it.
    assert!(cat.clear_flat_field(r.id).unwrap());
    assert!(!cat.clear_flat_field(r.id).unwrap());
    assert_eq!(cat.get_flat_field(r.id).unwrap(), None);
    assert_eq!(
        cat.clear_flat_field(RollId::new()).unwrap_err().code,
        ErrorCode::NotFound
    );
    cat.set_flat_field(other.id, &flat(0.6)).unwrap();
    cat.delete_roll(other.id).unwrap();
    assert_eq!(cat.get_flat_field(other.id).unwrap(), None);
}

#[test]
fn a_guess_is_replaced_by_a_confident_base_and_nothing_else_is() {
    let dir = TempDir::new("base-replace");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("R")).unwrap();
    let guess = [0.5, 0.4, 0.3];
    let good = [0.8, 0.5, 0.3];
    cat.improve_roll_film_base(r.id, guess, Some(0.2)).unwrap();

    // Another guess, even a better one, does not replace a guess; a confident base does.
    let (roll, change) = cat
        .improve_roll_film_base(r.id, [0.6, 0.4, 0.3], Some(0.4))
        .unwrap();
    assert_eq!(
        (change, roll.film_base_rgb),
        (BaseChange::Kept, Some(guess))
    );
    let (roll, change) = cat.improve_roll_film_base(r.id, good, Some(0.6)).unwrap();
    assert_eq!(change, BaseChange::Replaced);
    assert_eq!(
        (roll.film_base_rgb, roll.film_base_confidence),
        (Some(good), Some(0.6))
    );
    assert!(!roll.film_base_uncertain());
    // Now it is confident, nothing replaces it.
    let (_, change) = cat.improve_roll_film_base(r.id, guess, Some(0.9)).unwrap();
    assert_eq!(change, BaseChange::Kept);

    // A base picked by hand (no confidence) is never replaced, whatever the confidence.
    let mut by_hand = cat.get_roll(r.id).unwrap();
    by_hand.film_base_rgb = Some([0.7, 0.7, 0.7]);
    by_hand.film_base_confidence = None;
    cat.update_roll(&by_hand).unwrap();
    let (roll, change) = cat.improve_roll_film_base(r.id, good, Some(1.0)).unwrap();
    assert_eq!(
        (change, roll.film_base_rgb),
        (BaseChange::Kept, Some([0.7, 0.7, 0.7]))
    );

    // A guess is not replaced by a measurement that is a guess too, or has no confidence.
    by_hand.film_base_confidence = Some(0.1);
    cat.update_roll(&by_hand).unwrap();
    for confidence in [Some(0.3), None] {
        let (_, change) = cat.improve_roll_film_base(r.id, good, confidence).unwrap();
        assert_eq!(change, BaseChange::Kept, "{confidence:?}");
    }
}

#[test]
fn the_roll_base_is_stored_only_when_unset_and_checked() {
    let dir = TempDir::new("base-if-unset");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("R")).unwrap();
    let (stored, change) = cat
        .improve_roll_film_base(r.id, [0.8, 0.5, 0.3], Some(0.7))
        .unwrap();
    assert_eq!(change, BaseChange::Set);
    assert_eq!(stored.film_base_confidence, Some(0.7));
    let (again, change) = cat
        .improve_roll_film_base(r.id, [0.1, 0.1, 0.1], Some(0.99))
        .unwrap();
    assert_eq!(change, BaseChange::Kept, "a confident base stays");
    assert_eq!(again.film_base_rgb, Some([0.8, 0.5, 0.3]));
    for bad in [
        ([0.0, 0.5, 0.3], Some(0.5)),
        ([0.8, 0.5, 0.3], Some(1.2)),
        ([0.8, 0.5, 0.3], Some(f32::NAN)),
    ] {
        let err = cat.improve_roll_film_base(r.id, bad.0, bad.1).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput, "{bad:?}");
    }
    let err = cat
        .improve_roll_film_base(RollId::new(), [0.8, 0.5, 0.3], None)
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

/// Four rolls for the filter tests, each with frames, and a few tags.
struct Library {
    cat: Catalog,
    a: Vec<FrameId>,
    b: Vec<FrameId>,
    c: Vec<FrameId>,
    d: Vec<FrameId>,
    rolls: [RollId; 4],
}

fn filter_library(dir: &TempDir) -> Library {
    let mut cat = Catalog::open(&dir.db()).unwrap();
    // A: summer 2026, Nikon, pushed to 800.
    let a = cat.insert_roll(&roll("Summer")).unwrap();
    // B: winter, spans the new year, Leica, HP5 pushed to 1600.
    let mut winter = roll("Winter");
    winter.film_stock = Some("Ilford HP5".into());
    winter.camera = Some("Leica M6".into());
    winter.iso_box = Some(400);
    winter.iso_shot = Some(1600);
    winter.shot_from = Some("2025-12-20".into());
    winter.shot_to = Some("2026-01-05".into());
    let b = cat.insert_roll(&winter).unwrap();
    // C: no dates at all.
    let mut undated = roll("Undated");
    undated.film_stock = Some("Kodak Portra 160".into());
    undated.iso_box = Some(160);
    undated.iso_shot = None;
    undated.shot_from = None;
    undated.shot_to = None;
    let c = cat.insert_roll(&undated).unwrap();
    // D: only an end date, March 2026.
    let mut march = roll("March");
    march.film_stock = Some("Fuji Velvia 50".into());
    march.camera = Some("Pentax 67".into());
    march.iso_box = Some(50);
    march.iso_shot = Some(50);
    march.shot_from = None;
    march.shot_to = Some("2026-03".into());
    let d = cat.insert_roll(&march).unwrap();
    let mut frames = |roll: RollId, n: u32| -> Vec<FrameId> {
        (1..=n)
            .map(|i| cat.insert_frame(&frame(roll, i)).unwrap().id)
            .collect()
    };
    let (fa, fb, fc, fd) = (
        frames(a.id, 2),
        frames(b.id, 1),
        frames(c.id, 1),
        frames(d.id, 1),
    );
    Library {
        cat,
        a: fa,
        b: fb,
        c: fc,
        d: fd,
        rolls: [a.id, b.id, c.id, d.id],
    }
}

fn found(cat: &Catalog, query: &SearchQuery) -> Vec<FrameId> {
    let mut ids: Vec<FrameId> = cat
        .search_page(query, 0, 1000)
        .unwrap()
        .iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(
        cat.search_count(query).unwrap() as usize,
        ids.len(),
        "the count agrees with the page"
    );
    ids.sort();
    ids
}

fn sorted(groups: &[&Vec<FrameId>]) -> Vec<FrameId> {
    let mut ids: Vec<FrameId> = groups.iter().flat_map(|g| g.iter().copied()).collect();
    ids.sort();
    ids
}

#[test]
fn search_filters_by_text_film_camera_iso_and_roll() {
    let dir = TempDir::new("filters");
    let l = filter_library(&dir);
    let q = |f: fn(&mut SearchQuery)| {
        let mut query = SearchQuery::default();
        f(&mut query);
        found(&l.cat, &query)
    };
    assert_eq!(q(|_| {}).len(), 5, "no filter lists every frame");
    assert_eq!(q(|s| s.text = Some("portra".into())), sorted(&[&l.a, &l.c]));
    assert_eq!(
        q(|s| s.text = Some("  ".into())).len(),
        5,
        "blank text is no filter"
    );
    assert_eq!(
        q(|s| s.film_stock = Some("kodak PORTRA 400".into())),
        sorted(&[&l.a]),
        "film matches whole and ignores case"
    );
    assert_eq!(
        q(|s| s.camera = Some("nikon f3".into())),
        sorted(&[&l.a, &l.c])
    );
    assert_eq!(q(|s| s.iso = Some(800)), sorted(&[&l.a]), "shot ISO");
    assert_eq!(q(|s| s.iso = Some(400)), sorted(&[&l.a, &l.b]), "box ISO");
    assert_eq!(q(|s| s.iso = Some(1600)), sorted(&[&l.b]));
    assert!(q(|s| s.iso = Some(3200)).is_empty());
    assert_eq!(
        found(
            &l.cat,
            &SearchQuery {
                roll_id: Some(l.rolls[3]),
                ..SearchQuery::default()
            }
        ),
        sorted(&[&l.d])
    );
    // Everything given must match.
    assert_eq!(
        q(|s| {
            s.text = Some("kodak".into());
            s.camera = Some("Nikon F3".into());
            s.iso = Some(160);
        }),
        sorted(&[&l.c])
    );
    // Special characters are searched literally, never as FTS syntax.
    for text in ["\"", "portra OR", "NEAR(", "*", "a\"b"] {
        let query = SearchQuery {
            text: Some(text.into()),
            ..SearchQuery::default()
        };
        assert!(l.cat.search_page(&query, 0, 10).is_ok(), "{text:?}");
    }
}

#[test]
fn a_date_range_matches_rolls_whose_shooting_dates_overlap_it() {
    let dir = TempDir::new("dates");
    let l = filter_library(&dir);
    let between = |from: Option<&str>, to: Option<&str>| {
        found(
            &l.cat,
            &SearchQuery {
                date_from: from.map(str::to_owned),
                date_to: to.map(str::to_owned),
                ..SearchQuery::default()
            },
        )
    };
    // A shot 2026-07 to 2026-08-02: a month the roll only partly covers still matches.
    assert_eq!(between(Some("2026-08"), Some("2026-08")), sorted(&[&l.a]));
    assert_eq!(between(Some("2026-07"), Some("2026-07")), sorted(&[&l.a]));
    assert_eq!(
        between(Some("2026-07-15"), Some("2026-07-20")),
        sorted(&[&l.a])
    );
    assert_eq!(
        between(Some("2026-08-02"), None),
        sorted(&[&l.a]),
        "last day counts"
    );
    assert!(
        between(Some("2026-08-03"), None).is_empty(),
        "the day after does not"
    );
    assert_eq!(
        between(Some("2026-02"), Some("2026-06")),
        sorted(&[&l.d]),
        "only March's roll falls in February to June"
    );
    // B spans the new year.
    assert_eq!(between(Some("2025"), Some("2025")), sorted(&[&l.b]));
    assert_eq!(between(Some("2026-01"), Some("2026-01")), sorted(&[&l.b]));
    assert_eq!(between(None, Some("2025-12-19")), sorted(&[]));
    assert_eq!(between(None, Some("2025-12-20")), sorted(&[&l.b]));
    // D has only an end date: it counts as that month.
    assert_eq!(
        between(Some("2026-03-31"), None).len(),
        3,
        "D, A and not B or C"
    );
    assert_eq!(between(Some("2026-03"), Some("2026-03")), sorted(&[&l.d]));
    assert_eq!(between(Some("2026-04"), Some("2026-06")), sorted(&[]));
    // C has no dates and never matches a date filter.
    for range in [
        (Some("2000"), None),
        (None, Some("2100")),
        (Some("2000"), Some("2100")),
    ] {
        let ids = between(range.0, range.1);
        assert!(!ids.iter().any(|id| l.c.contains(id)), "{range:?}");
        assert_eq!(ids.len(), 4);
    }
    // Invalid or backwards ranges are errors, not empty results.
    for (from, to) in [
        (Some("2026-13"), None),
        (Some("2026-08"), Some("2026-07")),
        (None, Some("July")),
    ] {
        let query = SearchQuery {
            date_from: from.map(str::to_owned),
            date_to: to.map(str::to_owned),
            ..SearchQuery::default()
        };
        assert_eq!(
            l.cat.search_count(&query).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
}

#[test]
fn tags_all_needs_every_tag_own_or_from_the_roll_and_combines_with_other_filters() {
    let dir = TempDir::new("tags-all");
    let mut l = filter_library(&dir);
    let japan = l.cat.tag_ensure("Japan").unwrap().id;
    let sunset = l.cat.tag_ensure("sunset").unwrap().id;
    let family = l.cat.tag_ensure("family").unwrap().id;
    l.cat.tag_roll(l.rolls[0], &[japan], &[]).unwrap();
    l.cat.tag_frames(&[l.a[0]], &[sunset], &[]).unwrap();
    l.cat.tag_frames(&l.b, &[family], &[]).unwrap();
    let with = |tags: &[gtp_core::TagId]| {
        found(
            &l.cat,
            &SearchQuery {
                tags_all: tags.to_vec(),
                ..SearchQuery::default()
            },
        )
    };
    assert_eq!(with(&[japan]), sorted(&[&l.a]), "from the roll");
    assert_eq!(
        with(&[japan, sunset]),
        vec![l.a[0]],
        "from the roll and its own"
    );
    assert_eq!(
        with(&[sunset, sunset]),
        vec![l.a[0]],
        "duplicates count once"
    );
    assert!(with(&[family, japan]).is_empty());
    assert_eq!(with(&[]).len(), 5);
    assert!(
        with(&[gtp_core::TagId::new()]).is_empty(),
        "an unknown tag matches nothing"
    );
    // With text and a date range.
    let query = SearchQuery {
        text: Some("nikon".into()),
        tags_all: vec![japan],
        date_from: Some("2026-08".into()),
        ..SearchQuery::default()
    };
    assert_eq!(found(&l.cat, &query), sorted(&[&l.a]));
    // Text covers tag names, also inherited ones.
    assert_eq!(l.cat.search_frames("japan").unwrap().len(), 2);
}

#[test]
fn results_are_paged_best_match_first_and_otherwise_newest_roll_first() {
    let dir = TempDir::new("paging");
    let l = filter_library(&dir);
    let all = SearchQuery::default();
    let page = |q: &SearchQuery, offset, limit| -> Vec<FrameId> {
        l.cat
            .search_page(q, offset, limit)
            .unwrap()
            .iter()
            .map(|f| f.id)
            .collect()
    };
    // Newest roll first (D, C, B, A), roll order inside a roll.
    let order = [l.d[0], l.c[0], l.b[0], l.a[0], l.a[1]];
    assert_eq!(page(&all, 0, 10), order);
    assert_eq!(page(&all, 1, 2), order[1..3]);
    assert_eq!(page(&all, 4, 10), order[4..]);
    assert!(page(&all, 5, 10).is_empty());
    assert_eq!(l.cat.search_count(&all).unwrap(), 5);

    // With text the best match comes first: only one frame mentions "lighthouse" twice.
    let mut cat = l.cat;
    cat.set_frame_note(l.a[1], "lighthouse lighthouse lighthouse")
        .unwrap();
    cat.set_frame_note(l.d[0], "lighthouse").unwrap();
    let q = SearchQuery {
        text: Some("lighthouse".into()),
        ..SearchQuery::default()
    };
    let hits: Vec<FrameId> = cat
        .search_page(&q, 0, 10)
        .unwrap()
        .iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(hits, vec![l.a[1], l.d[0]]);
}

#[test]
fn tagging_by_name_creates_tags_and_is_all_or_nothing() {
    let dir = TempDir::new("apply");
    let mut l = filter_library(&dir);
    let names = |cat: &Catalog| -> Vec<String> {
        cat.tag_list()
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect()
    };
    let strings = |v: &[&str]| -> Vec<String> { v.iter().map(|s| (*s).to_owned()).collect() };

    l.cat
        .tag_apply_frames(&l.a, &strings(&["Sunset", "  Golden  Hour "]), &[])
        .unwrap();
    assert_eq!(names(&l.cat), ["Golden Hour", "Sunset"]);
    assert_eq!(l.cat.search_frames("golden").unwrap().len(), 2);
    // The same names in another spelling are the same tags.
    l.cat
        .tag_apply_frames(&l.a, &strings(&["SUNSET"]), &[])
        .unwrap();
    assert_eq!(names(&l.cat).len(), 2);
    // Removing: known names are removed, unknown ones are skipped, nothing is created.
    l.cat
        .tag_apply_frames(&l.a, &[], &strings(&["golden hour", "never-existed"]))
        .unwrap();
    assert_eq!(names(&l.cat).len(), 2);
    assert!(l.cat.search_frames("golden").unwrap().is_empty());
    assert_eq!(l.cat.search_frames("sunset").unwrap().len(), 2);

    // An unknown frame fails the whole change, and no tag is created on the way.
    let err = l
        .cat
        .tag_apply_frames(&[l.a[0], FrameId::new()], &strings(&["brand-new"]), &[])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    // So does a bad name, even after a good one.
    let err = l
        .cat
        .tag_apply_frames(&l.a, &strings(&["fine-name", "bell\u{7}"]), &[])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    let too_many: Vec<String> = (0..=gtp_catalog::MAX_TAG_NAMES)
        .map(|i| format!("t{i}"))
        .collect();
    assert_eq!(
        l.cat
            .tag_apply_frames(&l.a, &too_many, &[])
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(names(&l.cat), ["Golden Hour", "Sunset"], "nothing leaked");

    // Rolls: the tag counts for every frame, also ones added later.
    l.cat
        .tag_apply_roll(l.rolls[1], &strings(&["Winter Trip"]), &[])
        .unwrap();
    let later = l.cat.insert_frame(&frame(l.rolls[1], 2)).unwrap();
    assert_eq!(l.cat.search_frames("trip").unwrap().len(), 2);
    assert_eq!(l.cat.frame_tags(later.id).unwrap().from_roll.len(), 1);
    l.cat
        .tag_apply_roll(l.rolls[1], &[], &strings(&["winter TRIP"]))
        .unwrap();
    assert!(l.cat.search_frames("trip").unwrap().is_empty());
    let err = l
        .cat
        .tag_apply_roll(RollId::new(), &strings(&["ghost"]), &[])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(!names(&l.cat).contains(&"ghost".to_owned()));
}

#[test]
fn the_search_can_be_rebuilt_from_the_data() {
    let dir = TempDir::new("rebuild");
    let mut l = filter_library(&dir);
    let tag = l.cat.tag_ensure("keeper").unwrap().id;
    l.cat.tag_roll(l.rolls[0], &[tag], &[]).unwrap();
    let before = l.cat.search_frames("keeper").unwrap();
    assert_eq!(before.len(), 2);
    l.cat.close().unwrap();
    // Something else wipes the search rows.
    Connection::open(dir.db())
        .unwrap()
        .execute("DELETE FROM search", [])
        .unwrap();
    let mut cat = Catalog::open(&dir.db()).unwrap();
    assert!(cat.search_frames("keeper").unwrap().is_empty());
    cat.rebuild_search().unwrap();
    assert_eq!(cat.search_frames("keeper").unwrap().len(), 2);
    assert_eq!(cat.search_frames("nikon").unwrap().len(), 3);
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
fn several_recipes_are_saved_together_or_not_at_all() {
    let dir = TempDir::new("recipes");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("Summer")).unwrap();
    let (f1, f2) = (
        cat.insert_frame(&frame(r.id, 1)).unwrap(),
        cat.insert_frame(&frame(r.id, 2)).unwrap(),
    );
    let mut recipe = Recipe::default();
    recipe.tone.exposure_ev = 0.4;
    cat.save_recipes(&[(f1.id, recipe.clone()), (f2.id, recipe.clone())])
        .unwrap();
    for f in [f1.id, f2.id] {
        assert_eq!(cat.load_recipe(f).unwrap(), Some(recipe.clone()));
        assert!(cat.get_frame(f).unwrap().edited_at.is_some());
    }

    // An unknown frame in the list rolls the whole batch back.
    let mut other = Recipe::default();
    other.tone.exposure_ev = -0.4;
    let err = cat
        .save_recipes(&[(f1.id, other.clone()), (FrameId::new(), other)])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert_eq!(cat.load_recipe(f1.id).unwrap(), Some(recipe));

    cat.save_recipes(&[]).unwrap();
}

#[test]
fn a_v8_library_upgrades_to_v9_and_its_frames_have_no_capture_info() {
    let dir = TempDir::new("upgrade-v8");
    let (roll_id, frame_id) = (RollId::new(), FrameId::new());
    {
        let conn = Connection::open(dir.db()).unwrap();
        for sql in [
            include_str!("../migrations/0001_init.sql"),
            include_str!("../migrations/0002_search.sql"),
            include_str!("../migrations/0003_export_log.sql"),
            include_str!("../migrations/0004_roll_base_confidence.sql"),
            include_str!("../migrations/0005_roll_frame_size.sql"),
            include_str!("../migrations/0006_frame_scan.sql"),
            include_str!("../migrations/0007_flat_field.sql"),
            include_str!("../migrations/0008_roll_color_balance.sql"),
        ] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute(
            "INSERT INTO roll(id, name, created_at) VALUES (?1, 'Old', '2026-01-01T00:00:00Z')",
            [roll_id.to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO frame(id, roll_id, index_in_roll, kind, width, height, has_ir, note, created_at)
             VALUES (?1, ?2, 1, 'RgbTriplet', 10, 10, 0, '', '2026-01-01T00:00:00Z')",
            [frame_id.to_string(), roll_id.to_string()],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 8).unwrap();
    }
    let cat = Catalog::open(&dir.db()).unwrap();
    assert_eq!(cat.capture_info(frame_id).unwrap(), None);
    drop(cat);
    assert_eq!(user_version(&dir.db()), 9);
    let backup = backup_path(&dir.db(), 8);
    assert_eq!(user_version(&backup), 8, "the backup is the old schema");
    assert!(Connection::open(&backup)
        .unwrap()
        .prepare("SELECT capture_info FROM frame")
        .is_err());
}

#[test]
fn a_captured_frame_keeps_how_it_was_lit_and_other_frames_have_none() {
    let dir = TempDir::new("captured");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("Roll")).unwrap();
    let recipe = Recipe::default();
    let info = r#"{"v":1,"levels":[44,22,2]}"#;
    let captured = cat
        .insert_captured_frame(&frame(r.id, 1), &[], &recipe, info)
        .unwrap();
    assert_eq!(
        cat.capture_info(captured.id).unwrap().as_deref(),
        Some(info)
    );
    let imported = cat
        .insert_imported_frame(&frame(r.id, 1), &[], &recipe)
        .unwrap();
    assert_eq!(cat.capture_info(imported.id).unwrap(), None);
    let scanned = cat
        .insert_scanned_frame(&frame(r.id, 1), &[], &recipe, 1, "{}")
        .unwrap();
    assert_eq!(cat.capture_info(scanned.id).unwrap(), None);
    assert_eq!(
        cat.capture_info(FrameId::new()).unwrap_err().code,
        ErrorCode::NotFound
    );
    let err = cat
        .insert_captured_frame(&frame(RollId::new(), 1), &[], &recipe, info)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn a_scanned_frame_keeps_its_scanner_number_and_the_last_one_is_found() {
    let dir = TempDir::new("scanned");
    let mut cat = Catalog::open(&dir.db()).unwrap();
    let r = cat.insert_roll(&roll("Roll")).unwrap();
    let other = cat.insert_roll(&roll("Other")).unwrap();
    let recipe = Recipe::default();
    assert_eq!(
        cat.last_scan_number(r.id).unwrap(),
        None,
        "nothing scanned yet"
    );

    // A frame imported from a file has no scanner number.
    cat.insert_imported_frame(&frame(r.id, 1), &[], &recipe)
        .unwrap();
    assert_eq!(cat.last_scan_number(r.id).unwrap(), None);

    // Frames 5 and 6 scanned: they are the 2nd and 3rd of the roll, and 6 is the last number.
    for number in [5, 6] {
        let f = cat
            .insert_scanned_frame(&frame(r.id, 1), &[], &recipe, number, r#"{"dpi":2000}"#)
            .unwrap();
        assert_eq!(f.index_in_roll, number - 3);
    }
    assert_eq!(cat.last_scan_number(r.id).unwrap(), Some(6));
    assert_eq!(cat.last_scan_number(other.id).unwrap(), None, "per roll");

    // A frame scanned again keeps the highest number as the last one.
    cat.insert_scanned_frame(&frame(r.id, 1), &[], &recipe, 5, "{}")
        .unwrap();
    assert_eq!(cat.last_scan_number(r.id).unwrap(), Some(6));

    // A roll that does not exist is refused, as for an import.
    let err = cat
        .insert_scanned_frame(&frame(RollId::new(), 1), &[], &recipe, 1, "{}")
        .unwrap_err();
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

#[test]
fn exports_are_logged_per_frame_in_order_and_outlive_the_frame() {
    let dir = TempDir::new("export-log");
    let mut catalog = Catalog::open(&dir.db()).unwrap();
    let r = catalog.insert_roll(&roll("Exports")).unwrap();
    let a = catalog.insert_frame(&frame(r.id, 1)).unwrap().id;
    let b = FrameId::new();
    catalog.log_export(a, "tiff16", "/out/a.tif").unwrap();
    catalog.log_export(b, "jpeg", "/out/b.jpg").unwrap();
    catalog.log_export(a, "jpeg", "/out/a.jpg").unwrap();

    let of_a = catalog.exports_of(a).unwrap();
    assert_eq!(of_a.len(), 2);
    assert_eq!(
        (of_a[0].format.as_str(), of_a[0].path.as_str()),
        ("tiff16", "/out/a.tif")
    );
    assert_eq!(of_a[1].path, "/out/a.jpg");
    assert_eq!(of_a[0].frame_id, a.to_string());
    assert!(of_a[0].created_at.ends_with('Z') && of_a[0].created_at.len() == 20);
    assert!(catalog.exports_of(FrameId::new()).unwrap().is_empty());

    // The log is kept when the frame is deleted.
    catalog.delete_frames(&[a]).unwrap();
    assert_eq!(catalog.exports_of(a).unwrap().len(), 2);
}
