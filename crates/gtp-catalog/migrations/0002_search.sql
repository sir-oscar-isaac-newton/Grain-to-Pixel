-- Full-text search, one row per frame. `text` holds the roll's name, film, camera, ISO, dates and
-- notes, the frame note and the names of the frame's and its roll's tags; the catalog rewrites it in
-- the same transaction as every change to them (src/fts.rs). Diacritics are ignored when matching.
CREATE VIRTUAL TABLE search USING fts5(
    frame_id UNINDEXED,
    text,
    tokenize = 'unicode61 remove_diacritics 2'
);
