//! Local activation counters (specification section 14), kept in the library's `meta` table.
//! Nothing is sent anywhere: the user sees them in Settings and they go into the diagnostics zip
//! only when the user exports it.
//!
//! A counter must never fail the work it counts, so the `count_*` functions log and carry on when
//! the catalog cannot take the write (a read-only library, a full disk).

use gtp_catalog::Catalog;
use gtp_core::{AppError, ErrorCode};
use serde::Serialize;

use super::library::Library;

const FIRST_IMPORT_AT: &str = "first_import_at";
const FIRST_CONVERT_AT: &str = "first_convert_at";
const FIRST_EXPORT_AT: &str = "first_export_at";
const FRAMES_IMPORTED: &str = "frames_imported";
const FRAMES_EXPORTED: &str = "frames_exported";
const SCANS_FAILED: &str = "scans_failed";

/// The counters of one library. The `first_*` values are RFC 3339 times, set once.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Counters {
    pub first_import_at: Option<String>,
    pub first_convert_at: Option<String>,
    pub first_export_at: Option<String>,
    pub frames_imported: u32,
    pub frames_exported: u32,
    pub scans_failed: u32,
}

fn number(catalog: &Catalog, key: &str) -> Result<u32, AppError> {
    Ok(catalog
        .meta_get(key)?
        .and_then(|value| value.parse().ok())
        .unwrap_or(0))
}

fn add_one(catalog: &mut Catalog, key: &str) -> Result<(), AppError> {
    let next = number(catalog, key)?.saturating_add(1);
    catalog.meta_set(key, &next.to_string())
}

/// Sets `key` to the current time unless it already has a value.
fn set_once(catalog: &mut Catalog, key: &str) -> Result<(), AppError> {
    if catalog.meta_get(key)?.is_none() {
        let now = catalog.now()?;
        catalog.meta_set(key, &now)?;
    }
    Ok(())
}

fn best_effort(
    library: &Library,
    counter: &str,
    write: impl FnOnce(&mut Catalog) -> Result<(), AppError>,
) {
    if let Err(err) = library.write(write) {
        if err.code == ErrorCode::LibraryTooNew {
            tracing::debug!(counter, "counter not kept: the library is read-only");
        } else {
            tracing::warn!(counter, error = %err.message, "counter not kept");
        }
    }
}

/// The counters of `library`.
pub fn read(library: &Library) -> Result<Counters, AppError> {
    library.read(|c| {
        Ok(Counters {
            first_import_at: c.meta_get(FIRST_IMPORT_AT)?,
            first_convert_at: c.meta_get(FIRST_CONVERT_AT)?,
            first_export_at: c.meta_get(FIRST_EXPORT_AT)?,
            frames_imported: number(c, FRAMES_IMPORTED)?,
            frames_exported: number(c, FRAMES_EXPORTED)?,
            scans_failed: number(c, SCANS_FAILED)?,
        })
    })
}

/// One frame was imported.
pub fn count_import(library: &Library) {
    best_effort(library, FRAMES_IMPORTED, |c| {
        set_once(c, FIRST_IMPORT_AT)?;
        add_one(c, FRAMES_IMPORTED)
    });
}

/// A recipe was saved, which is the first time a negative is converted and edited. Reads first and
/// writes only once, because recipes are saved often.
pub fn count_convert(library: &Library) {
    if matches!(library.read(|c| c.meta_get(FIRST_CONVERT_AT)), Ok(Some(_))) {
        return;
    }
    best_effort(library, FIRST_CONVERT_AT, |c| set_once(c, FIRST_CONVERT_AT));
}

/// One frame was exported.
pub fn count_export(library: &Library) {
    best_effort(library, FRAMES_EXPORTED, |c| {
        set_once(c, FIRST_EXPORT_AT)?;
        add_one(c, FRAMES_EXPORTED)
    });
}

/// A scan ended in an error. The scanner jobs call this when they exist.
pub fn count_scan_failed(library: &Library) {
    best_effort(library, SCANS_FAILED, |c| add_one(c, SCANS_FAILED));
}

#[cfg(test)]
mod tests {
    use gtp_core::FrameId;

    use super::*;

    fn library(label: &str) -> (std::path::PathBuf, Library) {
        let dir = std::env::temp_dir().join(format!(
            "gtp-counters-{label}-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let library = Library::create(&dir.join("lib"), "Counted").unwrap();
        (dir, library)
    }

    #[test]
    fn a_new_library_has_nothing_counted() {
        let (dir, library) = library("new");
        assert_eq!(read(&library).unwrap(), Counters::default());
        drop(library);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn imports_and_exports_count_frames_and_stamp_the_first_time_once() {
        let (dir, library) = library("frames");
        count_import(&library);
        count_export(&library);
        // A marker no clock produces, so a second stamp would show.
        library
            .write(|c| {
                c.meta_set(FIRST_IMPORT_AT, "2020-01-01T00:00:00Z")?;
                c.meta_set(FIRST_EXPORT_AT, "2020-01-02T00:00:00Z")
            })
            .unwrap();
        count_import(&library);
        count_import(&library);
        count_export(&library);

        let counters = read(&library).unwrap();
        assert_eq!(counters.frames_imported, 3);
        assert_eq!(counters.frames_exported, 2);
        assert_eq!(
            counters.first_import_at.as_deref(),
            Some("2020-01-01T00:00:00Z")
        );
        assert_eq!(
            counters.first_export_at.as_deref(),
            Some("2020-01-02T00:00:00Z")
        );
        assert_eq!(counters.first_convert_at, None);
        drop(library);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_first_convert_is_stamped_once_and_failed_scans_add_up() {
        let (dir, library) = library("convert");
        count_convert(&library);
        let first = read(&library).unwrap().first_convert_at;
        assert!(first.is_some());
        library
            .write(|c| c.meta_set(FIRST_CONVERT_AT, "2020-01-03T00:00:00Z"))
            .unwrap();
        count_convert(&library);
        assert_eq!(
            read(&library).unwrap().first_convert_at.as_deref(),
            Some("2020-01-03T00:00:00Z")
        );

        count_scan_failed(&library);
        count_scan_failed(&library);
        assert_eq!(read(&library).unwrap().scans_failed, 2);
        drop(library);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_value_that_is_not_a_number_counts_as_zero() {
        let (dir, library) = library("junk");
        library
            .write(|c| c.meta_set(FRAMES_IMPORTED, "many"))
            .unwrap();
        count_import(&library);
        assert_eq!(read(&library).unwrap().frames_imported, 1);
        drop(library);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
