//! Searching frames (L-06): full-text over the `search` table combined with tag, roll, film, camera,
//! ISO and date filters in one query, paged, with a count.
//!
//! Every part of a [`SearchQuery`] is optional and all given parts must match. Text words are
//! quoted and prefix-matched, so FTS5 syntax typed by the user is searched literally.

use rusqlite::types::Value;
use rusqlite::{params_from_iter, Row};

use gtp_core::model::{check_iso, check_shot_range, partial_date_span};
use gtp_core::{AppError, ErrorCode, Frame, FrameId, SearchQuery};

use super::frame::{prefixed_columns, read};
use crate::db::db_error;
use crate::{fts, Catalog};

/// Most tags one search may require.
pub const MAX_SEARCH_TAGS: usize = 20;
/// Longest text, film or camera filter, in characters.
pub const MAX_SEARCH_CHARS: usize = 200;

/// The FTS5 query for `text`: each word quoted and prefix-matched, all words required.
fn fts_query(text: &str) -> String {
    text.split_whitespace()
        .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A roll's first day as a full ISO date, from a partial one (`2026` becomes `2026-01-01`).
fn first_day(date: &str) -> String {
    format!(
        "CASE length({date}) WHEN 4 THEN {date} || '-01-01' WHEN 7 THEN {date} || '-01' ELSE {date} END"
    )
}

/// A roll's last day as a full ISO date. A month ends on '-31' whatever its length: query dates
/// are valid days, so the comparison is still exact (no valid day lies between the 28th and the
/// 31st of a shorter month and the first of the next).
fn last_day(date: &str) -> String {
    format!(
        "CASE length({date}) WHEN 4 THEN {date} || '-12-31' WHEN 7 THEN {date} || '-31' ELSE {date} END"
    )
}

/// The part of the SQL shared by the page and the count.
struct Compiled {
    from_where: String,
    params: Vec<Value>,
    ranked: bool,
}

fn invalid(message: &str, details: String) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message.to_owned()).with_details(details)
}

fn clean_filter(text: &Option<String>, what: &str) -> Result<Option<String>, AppError> {
    match text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) if t.chars().count() > MAX_SEARCH_CHARS => Err(invalid(
            &format!("The {what} is too long."),
            format!("at most {MAX_SEARCH_CHARS} characters"),
        )),
        Some(t) => Ok(Some(t.to_owned())),
        None => Ok(None),
    }
}

fn compile(q: &SearchQuery) -> Result<Compiled, AppError> {
    let mut from = String::from("FROM frame f JOIN roll r ON r.id = f.roll_id");
    let mut wh: Vec<String> = Vec::new();
    let mut params: Vec<Value> = Vec::new();
    let mut ranked = false;

    if let Some(text) = clean_filter(&q.text, "search text")? {
        let query = fts_query(&text);
        from.push_str(" JOIN search ON search.rowid = f.rowid");
        wh.push("search MATCH ?".into());
        params.push(Value::Text(query));
        ranked = true;
    }
    if let Some(roll) = q.roll_id {
        wh.push("f.roll_id = ?".into());
        params.push(Value::Text(roll.to_string()));
    }
    if let Some(film) = clean_filter(&q.film_stock, "film name")? {
        wh.push("r.film_stock = ? COLLATE NOCASE".into());
        params.push(Value::Text(film));
    }
    if let Some(camera) = clean_filter(&q.camera, "camera name")? {
        wh.push("r.camera = ? COLLATE NOCASE".into());
        params.push(Value::Text(camera));
    }
    check_iso(q.iso)?;
    if let Some(iso) = q.iso {
        wh.push("(r.iso_box = ? OR r.iso_shot = ?)".into());
        params.extend(std::iter::repeat_n(Value::Integer(i64::from(iso)), 2));
    }

    let mut tags = q.tags_all.clone();
    tags.sort();
    tags.dedup();
    if tags.len() > MAX_SEARCH_TAGS {
        return Err(invalid(
            "Too many tags in one search.",
            format!("{} (at most {MAX_SEARCH_TAGS})", tags.len()),
        ));
    }
    for tag in tags {
        // The frames with this tag (their own, or through their roll) as a set to probe, which
        // starts from the tag's index instead of testing every frame.
        wh.push(
            "f.id IN (SELECT frame_id FROM frame_tag WHERE tag_id = ?
                      UNION SELECT fr.id FROM roll_tag rt JOIN frame fr ON fr.roll_id = rt.roll_id
                      WHERE rt.tag_id = ?)"
                .into(),
        );
        params.extend(std::iter::repeat_n(Value::Text(tag.to_string()), 2));
    }

    // Dates: a roll matches when its shooting dates overlap the range. A roll with only one of its
    // two dates is taken as that single span; with neither, the comparisons are NULL and it never
    // matches.
    let (from_date, to_date) = (
        clean_filter(&q.date_from, "start date")?,
        clean_filter(&q.date_to, "end date")?,
    );
    check_shot_range(from_date.as_deref(), to_date.as_deref())?;
    if let Some(date) = &from_date {
        wh.push(format!(
            "{} >= ?",
            last_day("coalesce(r.shot_to, r.shot_from)")
        ));
        params.push(Value::Text(partial_date_span(date)?.0));
    }
    if let Some(date) = &to_date {
        wh.push(format!(
            "{} <= ?",
            first_day("coalesce(r.shot_from, r.shot_to)")
        ));
        params.push(Value::Text(partial_date_span(date)?.1));
    }

    let where_sql = if wh.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", wh.join(" AND "))
    };
    Ok(Compiled {
        from_where: format!("{from}{where_sql}"),
        params,
        ranked,
    })
}

impl Catalog {
    /// One page of the frames matching `query`: best text match first when there is text, otherwise
    /// newest roll first and roll order.
    pub fn search_page(
        &self,
        query: &SearchQuery,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Frame>, AppError> {
        let c = compile(query)?;
        let rank = if c.ranked { "search.rank, " } else { "" };
        let sql = format!(
            "SELECT {} {} ORDER BY {rank}r.created_at DESC, r.rowid DESC, f.index_in_roll, f.rowid
             LIMIT ? OFFSET ?",
            prefixed_columns(),
            c.from_where
        );
        let mut params = c.params;
        params.extend([
            Value::Integer(i64::from(limit)),
            Value::Integer(i64::from(offset)),
        ]);
        let mut stmt = self.conn.prepare(&sql).map_err(db_error)?;
        let frames = stmt
            .query_map(params_from_iter(params), |row: &Row<'_>| read(row))
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error);
        frames
    }

    /// How many frames match `query`, over all pages.
    pub fn search_count(&self, query: &SearchQuery) -> Result<u32, AppError> {
        let c = compile(query)?;
        self.conn
            .query_row(
                &format!("SELECT count(*) {}", c.from_where),
                params_from_iter(c.params),
                |row| row.get(0),
            )
            .map_err(db_error)
    }

    /// The frames matching every word of `text`, best match first (no other filters, no paging).
    pub fn search_frames(&self, text: &str) -> Result<Vec<FrameId>, AppError> {
        let query = SearchQuery {
            text: Some(text.to_owned()),
            ..SearchQuery::default()
        };
        if fts_query(text).is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .search_page(&query, 0, u32::MAX)?
            .into_iter()
            .map(|f| f.id)
            .collect())
    }

    /// Rebuilds every search row from the rolls, frames and tags. The search table is kept up to
    /// date by every change; this repairs it after something else wrote to the database.
    pub fn rebuild_search(&mut self) -> Result<(), AppError> {
        let tx = self.conn.transaction().map_err(db_error)?;
        fts::rebuild_all(&tx)?;
        tx.commit().map_err(db_error)
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

    #[test]
    fn partial_roll_dates_expand_to_their_first_and_last_day() {
        assert_eq!(
            first_day("x"),
            "CASE length(x) WHEN 4 THEN x || '-01-01' WHEN 7 THEN x || '-01' ELSE x END"
        );
        assert!(last_day("x").contains("'-12-31'") && last_day("x").contains("'-31'"));
    }

    #[test]
    fn invalid_queries_are_rejected_before_any_sql_runs() {
        let bad = |q: SearchQuery| compile(&q).err().map(|e| e.code);
        let base = SearchQuery::default;
        assert_eq!(
            bad(SearchQuery {
                date_from: Some("2026-13".into()),
                ..base()
            }),
            Some(ErrorCode::InvalidInput)
        );
        assert_eq!(
            bad(SearchQuery {
                date_from: Some("2026-08".into()),
                date_to: Some("2026-07".into()),
                ..base()
            }),
            Some(ErrorCode::InvalidInput)
        );
        assert_eq!(
            bad(SearchQuery {
                iso: Some(0),
                ..base()
            }),
            Some(ErrorCode::InvalidInput)
        );
        assert_eq!(
            bad(SearchQuery {
                text: Some("x".repeat(MAX_SEARCH_CHARS + 1)),
                ..base()
            }),
            Some(ErrorCode::InvalidInput)
        );
        let many = (0..=MAX_SEARCH_TAGS)
            .map(|_| gtp_core::TagId::new())
            .collect();
        assert_eq!(
            bad(SearchQuery {
                tags_all: many,
                ..base()
            }),
            Some(ErrorCode::InvalidInput)
        );
        assert!(compile(&base()).is_ok());
    }
}
