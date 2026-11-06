-- The flat field of a roll of camera triplets: the smoothed light of a bare-light capture, which the
-- roll's triplet frames are divided by when they are loaded. At most one for a roll, removed with it.
-- `light` is the grid as little-endian f32, red, green and blue of each cell, row by row; `sources`
-- is a JSON list of the three file names. Existing rolls have none.
CREATE TABLE flat_field (
    roll_id    TEXT PRIMARY KEY REFERENCES roll(id) ON DELETE CASCADE,
    grid_w     INTEGER NOT NULL,
    grid_h     INTEGER NOT NULL,
    frame_w    INTEGER NOT NULL,
    frame_h    INTEGER NOT NULL,
    light      BLOB NOT NULL,
    sources    TEXT NOT NULL,
    created_at TEXT NOT NULL
);