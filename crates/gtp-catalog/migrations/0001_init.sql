-- Initial schema (docs/specification.md section 5.2). Timestamps are RFC 3339 UTC text.

CREATE TABLE roll (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    film_stock  TEXT,
    format      TEXT NOT NULL DEFAULT '35mm',
    camera      TEXT,
    iso_box     INTEGER,
    iso_shot    INTEGER,
    shot_from   TEXT,               -- partial ISO date: 2026, 2026-07 or 2026-07-14
    shot_to     TEXT,
    notes       TEXT NOT NULL DEFAULT '',
    film_base_r REAL,
    film_base_g REAL,
    film_base_b REAL,
    created_at  TEXT NOT NULL
);

CREATE TABLE frame (
    id            TEXT PRIMARY KEY,
    roll_id       TEXT NOT NULL REFERENCES roll(id) ON DELETE CASCADE,
    index_in_roll INTEGER NOT NULL,
    kind          TEXT NOT NULL,
    width         INTEGER NOT NULL,
    height        INTEGER NOT NULL,
    has_ir        INTEGER NOT NULL,
    note          TEXT NOT NULL DEFAULT '',
    created_at    TEXT NOT NULL,
    edited_at     TEXT
);

CREATE TABLE source_file (
    frame_id TEXT NOT NULL REFERENCES frame(id) ON DELETE CASCADE,
    role     TEXT NOT NULL,
    rel_path TEXT NOT NULL,
    sha256   TEXT NOT NULL,
    bytes    INTEGER NOT NULL,
    PRIMARY KEY (frame_id, role)
);

CREATE TABLE recipe (
    frame_id   TEXT PRIMARY KEY REFERENCES frame(id) ON DELETE CASCADE,
    schema_v   INTEGER NOT NULL,
    json       TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- `name` keeps the spelling it was typed with; `name_key` (gtp_core Tag::key: normalized and
-- lower-cased across Unicode) makes names unique case-insensitively, also for accented letters.
CREATE TABLE tag (
    id       TEXT PRIMARY KEY,
    name     TEXT NOT NULL,
    name_key TEXT NOT NULL UNIQUE
);

CREATE TABLE frame_tag (
    frame_id TEXT NOT NULL REFERENCES frame(id) ON DELETE CASCADE,
    tag_id   TEXT NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
    PRIMARY KEY (frame_id, tag_id)
);

-- A roll's tags count for every frame on it, also frames imported later.
CREATE TABLE roll_tag (
    roll_id TEXT NOT NULL REFERENCES roll(id) ON DELETE CASCADE,
    tag_id  TEXT NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
    PRIMARY KEY (roll_id, tag_id)
);

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT
);

CREATE INDEX idx_frame_roll ON frame(roll_id, index_in_roll);
CREATE INDEX idx_frame_tag_tag ON frame_tag(tag_id);
CREATE INDEX idx_roll_tag_tag ON roll_tag(tag_id);
