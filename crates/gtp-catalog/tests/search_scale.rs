//! Search at the size the app is built for (50,000 frames, spec section 11: at most 100 ms), and
//! reading while another connection writes (WAL).

// Test code (convention: tests may unwrap); clippy.toml only exempts `#[test]` functions, not the
// helpers of an integration-test file.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gtp_catalog::Catalog;
use gtp_core::{FilmFormat, Frame, FrameId, FrameKind, Roll, RollId, SearchQuery, TagId};
use rusqlite::{params, Connection};

const ROLLS: usize = 1_000;
const FRAMES_PER_ROLL: usize = 50;
const BUDGET: Duration = Duration::from_millis(100);
const FILMS: [&str; 10] = [
    "Kodak Portra 400",
    "Kodak Portra 160",
    "Kodak Ektar 100",
    "Ilford HP5 Plus",
    "Ilford Delta 3200",
    "Fuji Velvia 50",
    "Fuji Pro 400H",
    "Cinestill 800T",
    "Kodak Gold 200",
    "Agfa Vista 200",
];
const CAMERAS: [&str; 8] = [
    "Nikon F3",
    "Nikon FM2",
    "Leica M6",
    "Pentax K1000",
    "Canon AE-1",
    "Olympus OM-1",
    "Minolta X-700",
    "Contax T2",
];

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gtp-scale-{label}-{}-{}",
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

/// A library of `ROLLS * FRAMES_PER_ROLL` frames, written straight into the tables in one
/// transaction (as a bulk import would), then indexed with `rebuild_search`. Returns the catalog
/// and the ids of two tags that many frames share.
fn big_library(dir: &TempDir) -> (Catalog, [TagId; 2]) {
    Catalog::open(&dir.db()).unwrap().close().unwrap();
    let tags = [TagId::new(), TagId::new()];
    {
        let mut conn = Connection::open(dir.db()).unwrap();
        let tx = conn.transaction().unwrap();
        for (i, (id, name)) in [(tags[0], "japan"), (tags[1], "street")]
            .into_iter()
            .enumerate()
        {
            tx.execute(
                "INSERT INTO tag(id, name, name_key) VALUES (?1, ?2, ?2)",
                params![
                    id.to_string(),
                    format!("{name}{}", if i == 2 { "x" } else { "" })
                ],
            )
            .unwrap();
        }
        // 200 more tags, so tag joins do not run on a tiny table.
        for n in 0..200 {
            tx.execute(
                "INSERT INTO tag(id, name, name_key) VALUES (?1, ?2, ?2)",
                params![TagId::new().to_string(), format!("tag{n}")],
            )
            .unwrap();
        }
        let mut roll = tx
            .prepare(
                "INSERT INTO roll(id, name, film_stock, camera, iso_box, iso_shot, shot_from,
                                  shot_to, notes, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )
            .unwrap();
        let mut frame = tx
            .prepare(
                "INSERT INTO frame(id, roll_id, index_in_roll, kind, width, height, has_ir, note,
                                   created_at)
                 VALUES (?1, ?2, ?3, 'ScanRgbIr', 5616, 3744, 1, ?4, ?5)",
            )
            .unwrap();
        let mut roll_tag = tx
            .prepare("INSERT INTO roll_tag(roll_id, tag_id) VALUES (?1, ?2)")
            .unwrap();
        let mut frame_tag = tx
            .prepare("INSERT INTO frame_tag(frame_id, tag_id) VALUES (?1, ?2)")
            .unwrap();
        for r in 0..ROLLS {
            let id = RollId::new().to_string();
            let (year, month) = (2020 + r % 7, 1 + r % 12);
            let created = format!("2026-01-01T00:{:02}:{:02}Z", r / 60 % 60, r % 60);
            roll.execute(params![
                id,
                format!("Roll {r}"),
                FILMS[r % FILMS.len()],
                CAMERAS[r % CAMERAS.len()],
                [100, 200, 400, 800][r % 4],
                [400, 800, 1600][r % 3],
                format!("{year}-{month:02}"),
                format!("{year}-{month:02}-28"),
                format!("shot around town number {r}"),
                created,
            ])
            .unwrap();
            // Every 7th roll is tagged "japan"; every 3rd roll also gets one of the filler tags.
            if r % 7 == 0 {
                roll_tag.execute(params![id, tags[0].to_string()]).unwrap();
            }
            for f in 0..FRAMES_PER_ROLL {
                let frame_id = FrameId::new().to_string();
                frame
                    .execute(params![
                        frame_id,
                        id,
                        f as i64 + 1,
                        if f % 9 == 0 { "harbour at dusk" } else { "" },
                        created
                    ])
                    .unwrap();
                if (r + f) % 10 == 0 {
                    frame_tag
                        .execute(params![frame_id, tags[1].to_string()])
                        .unwrap();
                }
            }
        }
        drop((roll, frame, roll_tag, frame_tag));
        tx.commit().unwrap();
    }
    let mut cat = Catalog::open(&dir.db()).unwrap();
    cat.rebuild_search().unwrap();
    (cat, tags)
}

/// The best of a few runs of page plus count, so one slow scheduling hiccup does not fail it.
fn best_of(runs: usize, cat: &Catalog, query: &SearchQuery) -> (Duration, u32) {
    let mut best = Duration::MAX;
    let mut total = 0;
    for _ in 0..runs {
        let start = Instant::now();
        let page = cat.search_page(query, 0, 50).unwrap();
        total = cat.search_count(query).unwrap();
        best = best.min(start.elapsed());
        assert_eq!(page.len(), total.min(50) as usize);
    }
    (best, total)
}

#[test]
fn search_over_50000_frames_takes_at_most_100_ms() {
    let dir = TempDir::new("50k");
    let (cat, tags) = big_library(&dir);
    assert_eq!(
        cat.count_frames(None).unwrap() as usize,
        ROLLS * FRAMES_PER_ROLL
    );

    let text = |t: &str| SearchQuery {
        text: Some(t.into()),
        ..SearchQuery::default()
    };
    let queries: Vec<(&str, SearchQuery)> = vec![
        ("everything", SearchQuery::default()),
        ("broad text", text("portra")),
        ("text and camera word", text("portra nikon")),
        ("one roll's name", text("roll 123")),
        ("frame note", text("harbour")),
        (
            "two tags",
            SearchQuery {
                tags_all: tags.to_vec(),
                ..SearchQuery::default()
            },
        ),
        (
            "film, camera and a date range",
            SearchQuery {
                film_stock: Some("Kodak Portra 400".into()),
                camera: Some("Nikon F3".into()),
                date_from: Some("2022".into()),
                date_to: Some("2024-06".into()),
                ..SearchQuery::default()
            },
        ),
        (
            "text, tag, iso and dates",
            SearchQuery {
                text: Some("town".into()),
                tags_all: vec![tags[0]],
                iso: Some(800),
                date_from: Some("2020-06".into()),
                ..SearchQuery::default()
            },
        ),
    ];
    let mut slowest = ("", Duration::ZERO);
    for (name, query) in &queries {
        let (took, total) = best_of(5, &cat, query);
        eprintln!("{name}: {took:?} for {total} matches");
        assert!(total > 0, "{name} should match something");
        if took > slowest.1 {
            slowest = (name, took);
        }
        assert!(
            took <= BUDGET,
            "{name} took {took:?}, the budget is {BUDGET:?}"
        );
    }
    eprintln!("slowest: {} at {:?}", slowest.0, slowest.1);
}

#[test]
fn reading_goes_on_while_another_connection_writes() {
    let dir = TempDir::new("wal");
    let mut writer = Catalog::open(&dir.db()).unwrap();
    let roll = writer
        .insert_roll(&Roll {
            id: RollId::new(),
            name: "Busy".into(),
            film_stock: Some("Kodak Portra 400".into()),
            format: FilmFormat::Mm35,
            camera: None,
            iso_box: None,
            iso_shot: None,
            shot_from: None,
            shot_to: None,
            notes: String::new(),
            film_base_rgb: None,
            film_base_confidence: None,
            created_at: String::new(),
        })
        .unwrap();
    let reader = Catalog::open_reader(&dir.db()).unwrap();

    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    let writes = std::thread::spawn(move || {
        for i in 1..=200u32 {
            writer
                .insert_frame(&Frame {
                    id: FrameId::new(),
                    roll_id: roll.id,
                    index_in_roll: i,
                    kind: FrameKind::ScanRgbIr,
                    width: 100,
                    height: 100,
                    has_ir: false,
                    note: format!("frame {i}"),
                    created_at: String::new(),
                    edited_at: None,
                })
                .unwrap();
        }
        flag.store(true, Ordering::Release);
        writer
    });

    let query = SearchQuery {
        text: Some("portra".into()),
        ..SearchQuery::default()
    };
    let (mut reads, mut last) = (0u32, 0u32);
    while !done.load(Ordering::Acquire) {
        // Every read succeeds, never "database is locked", and never sees a count go backwards.
        let total = reader.search_count(&query).unwrap();
        let page = reader.search_page(&query, 0, 20).unwrap();
        assert!(total >= last, "{total} after {last}");
        assert!(page.len() <= 20);
        last = total;
        reads += 1;
    }
    let _writer = writes.join().unwrap();
    assert_eq!(reader.search_count(&query).unwrap(), 200);
    assert!(reads > 0);
    eprintln!("{reads} reads while 200 frames were written");
}
