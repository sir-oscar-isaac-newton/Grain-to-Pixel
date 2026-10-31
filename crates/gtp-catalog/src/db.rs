//! Opening a catalog: pragmas, the newer-schema check, the pre-migration backup, migrations, and the
//! integrity check after an unclean exit (docs/specification.md section 5.3).

use std::path::{Path, PathBuf};

use gtp_core::{AppError, ErrorCode};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use rusqlite_migration::{Migrations, M};

/// Forward-only migrations; the database's `user_version` is the number applied.
const MIGRATION_SQL: [&str; 3] = [
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_search.sql"),
    include_str!("../migrations/0003_export_log.sql"),
];

/// The schema version this build writes.
pub const SCHEMA_VERSION: u32 = MIGRATION_SQL.len() as u32;

fn migrations() -> Migrations<'static> {
    Migrations::new(MIGRATION_SQL.iter().map(|sql| M::up(sql)).collect())
}

/// `meta` key set while a session has the catalog open; still set at open means a crash.
const SESSION_OPEN_KEY: &str = "session_open";

/// Maps a database error to `Db`, with the SQLite message as details.
pub(crate) fn db_error(err: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Db, "The library database could not be used.")
        .with_details(err.to_string())
}

/// A connection to the library database. [`Catalog::open`] gives the writer: migrated, writes run
/// in transactions, and dropping it marks the session as cleanly closed. [`Catalog::open_reader`]
/// gives a read-only connection beside it (WAL lets it read while the writer writes).
pub struct Catalog {
    pub(crate) conn: Connection,
    path: PathBuf,
    recovered: bool,
    writer: bool,
}

/// The backup written before migrating from schema `version`.
pub fn backup_path(db: &Path, version: u32) -> PathBuf {
    let mut name = db.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".bak-v{version}"));
    db.with_file_name(name)
}

fn user_version(conn: &Connection) -> Result<u32, AppError> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(db_error)
}

fn configure(conn: &Connection) -> Result<(), AppError> {
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(db_error)?;
    let mode: String = conn
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
        .map_err(db_error)?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(db_error(format!("journal mode is {mode}, not WAL")));
    }
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(db_error)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(db_error)
}

/// Copies the database (including what is still in the WAL) to `target`, replacing an older copy.
fn backup_to(conn: &Connection, target: &Path) -> Result<(), AppError> {
    if target.exists() {
        std::fs::remove_file(target).map_err(|err| AppError::from_io(&err))?;
    }
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy()])
        .map(|_| ())
        .map_err(db_error)
}

impl Catalog {
    /// Opens (or creates) the catalog at `path` and brings it to [`SCHEMA_VERSION`]. A database
    /// with migrations is first copied to `library.db.bak-v<N>`. A database from a newer app
    /// version returns `LibraryTooNew` and is not touched.
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let mut conn = Connection::open(path).map_err(db_error)?;
        let found = user_version(&conn)?;
        if found > SCHEMA_VERSION {
            return Err(AppError::new(
                ErrorCode::LibraryTooNew,
                "This library was made by a newer version of Grain to Pixel.",
            )
            .with_details(format!(
                "schema {found}, this version reads up to {SCHEMA_VERSION}"
            )));
        }
        configure(&conn)?;
        if found > 0 && found < SCHEMA_VERSION {
            backup_to(&conn, &backup_path(path, found))?;
        }
        migrations().to_latest(&mut conn).map_err(db_error)?;

        let mut catalog = Self {
            conn,
            path: path.to_path_buf(),
            recovered: false,
            writer: true,
        };
        if catalog.meta_get(SESSION_OPEN_KEY)?.as_deref() == Some("1") {
            catalog.recovered = true;
            catalog.integrity_check()?;
        }
        catalog.meta_set(SESSION_OPEN_KEY, "1")?;
        Ok(catalog)
    }

    /// Opens an existing catalog read-only: no migration, no session flag, every write fails. Used
    /// for the read connection next to the writer, and alone for a library from a newer app version
    /// (whose schema this version may only partly understand).
    pub fn open_reader(path: &Path) -> Result<Self, AppError> {
        if !path.is_file() {
            return Err(AppError::new(
                gtp_core::ErrorCode::NotFound,
                "The library database is missing.",
            )
            .with_details(path.display().to_string()));
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        conn.pragma_update(None, "query_only", "ON")
            .map_err(db_error)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
            recovered: false,
            writer: false,
        })
    }

    /// The schema version stored in the database (above [`SCHEMA_VERSION`] for a newer library).
    pub fn schema_version(&self) -> Result<u32, AppError> {
        user_version(&self.conn)
    }

    /// True for the writer, false for a reader.
    pub fn is_writer(&self) -> bool {
        self.writer
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// True when the previous session did not close cleanly (the integrity check then ran and
    /// passed during `open`).
    pub fn recovered_from_unclean_exit(&self) -> bool {
        self.recovered
    }

    /// Runs SQLite's integrity and foreign-key checks; `Db` with the findings when either fails.
    pub fn integrity_check(&self) -> Result<(), AppError> {
        let damaged = || {
            AppError::new(
                ErrorCode::Db,
                "The library database is damaged. Restore it from a backup.",
            )
        };
        let mut stmt = self
            .conn
            .prepare("PRAGMA integrity_check")
            .map_err(db_error)?;
        let findings: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)?;
        if findings != ["ok"] {
            return Err(damaged().with_details(findings.join("; ")));
        }
        let broken_link: Option<String> = self
            .conn
            .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
            .optional()
            .map_err(db_error)?;
        if let Some(table) = broken_link {
            return Err(damaged().with_details(format!("broken reference in table {table}")));
        }
        Ok(())
    }

    /// Closes the catalog, marking the session as cleanly ended.
    pub fn close(mut self) -> Result<(), AppError> {
        self.end_session()
    }

    fn end_session(&mut self) -> Result<(), AppError> {
        self.meta_set(SESSION_OPEN_KEY, "0")
    }
}

impl Drop for Catalog {
    fn drop(&mut self) {
        // Best effort: if it fails, the next open runs the integrity check, which is harmless.
        if self.writer {
            let _ = self.end_session();
        }
    }
}
