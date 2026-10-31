//! The library catalog (docs/specification.md sections 5.2 and 5.3): the SQLite database with its
//! forward-only migrations, the backup taken before migrating, the integrity check after an unclean
//! exit, and one repository per entity. Search text is kept in step in the same transaction as
//! every change it depends on.

mod db;
mod fts;
mod repo;

pub use db::{backup_path, Catalog, SCHEMA_VERSION};
pub use repo::{
    ExportRecord, FrameTags, RecordedOriginal, MAX_NAME_CHARS, MAX_NOTE_CHARS, MAX_SEARCH_CHARS,
    MAX_SEARCH_TAGS, MAX_TAGGED_FRAMES, MAX_TAG_NAMES,
};
