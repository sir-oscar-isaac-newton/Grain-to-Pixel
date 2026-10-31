-- One row per exported file (written by the export job, M3). Kept when the frame is deleted.
CREATE TABLE export_log (
    id         INTEGER PRIMARY KEY,
    frame_id   TEXT,
    format     TEXT,
    path       TEXT,
    created_at TEXT
);
