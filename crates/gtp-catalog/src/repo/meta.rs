//! Key-value settings of the library (`meta`): its name, first-use times, counters.

use rusqlite::{params, OptionalExtension};

use gtp_core::AppError;

use crate::db::db_error;
use crate::Catalog;

impl Catalog {
    pub fn meta_get(&self, key: &str) -> Result<Option<String>, AppError> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map(Option::flatten)
            .map_err(db_error)
    }

    pub fn meta_set(&mut self, key: &str, value: &str) -> Result<(), AppError> {
        self.conn
            .execute(
                "INSERT INTO meta(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(|_| ())
            .map_err(db_error)
    }
}
