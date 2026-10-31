//! Full-text search over frames. Every word must match (as a word prefix), accents ignored; the
//! words are quoted, so FTS5 syntax typed by the user is searched literally. Query language and
//! ranking belong to L-06.

use gtp_core::{AppError, FrameId};

use crate::db::db_error;
use crate::Catalog;

/// The FTS5 query for `text`: each word quoted and prefix-matched, all words required.
fn fts_query(text: &str) -> String {
    text.split_whitespace()
        .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

impl Catalog {
    /// The frames matching every word of `text`, best match first.
    pub fn search_frames(&self, text: &str) -> Result<Vec<FrameId>, AppError> {
        let query = fts_query(text);
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare("SELECT frame_id FROM search WHERE search MATCH ?1 ORDER BY rank")
            .map_err(db_error)?;
        let ids = stmt
            .query_map([query], |row| row.get::<_, String>(0))
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?;
        ids.iter().map(|id| id.parse()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_quoted_and_prefix_matched() {
        assert_eq!(fts_query("  portra  400 "), "\"portra\"* \"400\"*");
        assert_eq!(fts_query("a\"b OR c"), "\"a\"\"b\"* \"OR\"* \"c\"*");
        assert_eq!(fts_query("   "), "");
    }
}
