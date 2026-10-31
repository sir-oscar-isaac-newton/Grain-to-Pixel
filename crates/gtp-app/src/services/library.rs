//! An open library: its folder, a writer and a reader connection to its catalog. A library made by
//! a newer app version opens read-only (spec section 5.3): reads work, writes return an error.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use gtp_catalog::Catalog;
use gtp_core::{AppError, ErrorCode};
use serde::Serialize;

use crate::library_fs::{self, DB_FILE};
use crate::paths::{canonical_dir, display_path, resolve_inside};

/// Longest library name accepted, in characters.
pub const MAX_LIBRARY_NAME_CHARS: usize = 200;
const NAME_KEY: &str = "library_name";
const CREATED_KEY: &str = "library_created_at";

/// What the UI shows about the open library.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LibraryDetails {
    /// The library folder, for display.
    pub path: String,
    pub name: String,
    pub created_at: Option<String>,
    /// True for a library made by a newer app version: it can be browsed but not changed.
    pub read_only: bool,
    pub schema_version: u32,
    /// Bytes free on the library's drive; `None` when the system cannot tell.
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub free_bytes: Option<u64>,
}

pub struct Library {
    root: PathBuf,
    writer: Option<Mutex<Catalog>>,
    reader: Mutex<Catalog>,
}

fn lock(catalog: &Mutex<Catalog>) -> MutexGuard<'_, Catalog> {
    catalog.lock().unwrap_or_else(PoisonError::into_inner)
}

fn clean_library_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The library needs a name.",
        ));
    }
    if name.chars().count() > MAX_LIBRARY_NAME_CHARS {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The library name is too long.",
        ));
    }
    Ok(name.to_owned())
}

impl Library {
    /// Creates a library in `path`, which must be missing or an empty folder.
    pub fn create(path: &Path, name: &str) -> Result<Self, AppError> {
        let name = clean_library_name(name)?;
        if path.exists() {
            if library_fs::is_library(path) {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    "That folder already holds a library. Open it instead.",
                )
                .with_details(path.display().to_string()));
            }
            let mut entries = std::fs::read_dir(path).map_err(|err| AppError::from_io(&err))?;
            if entries.next().is_some() {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    "Choose an empty folder for a new library.",
                )
                .with_details(path.display().to_string()));
            }
        }
        std::fs::create_dir_all(path).map_err(|err| AppError::from_io(&err))?;
        let root = canonical_dir(path)?;
        library_fs::ensure_layout(&root)?;
        let mut catalog = Catalog::open(&root.join(DB_FILE))?;
        catalog.meta_set(NAME_KEY, &name)?;
        let now = catalog.now()?;
        catalog.meta_set(CREATED_KEY, &now)?;
        drop(catalog);
        Self::open(&root)
    }

    /// Opens the library in `path`. Missing cache folders are recreated; a library from a newer
    /// app version opens read-only and is left untouched.
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let root = canonical_dir(path)?;
        if !library_fs::is_library(&root) {
            return Err(AppError::new(
                ErrorCode::NotFound,
                "That folder is not a Grain to Pixel library.",
            )
            .with_details(path.display().to_string()));
        }
        let db = root.join(DB_FILE);
        let writer = match Catalog::open(&db) {
            Ok(writer) => {
                library_fs::ensure_layout(&root)?;
                Some(Mutex::new(writer))
            }
            Err(err) if err.code == ErrorCode::LibraryTooNew => None,
            Err(err) => return Err(err),
        };
        let reader = Mutex::new(Catalog::open_reader(&db)?);
        Ok(Self {
            root,
            writer,
            reader,
        })
    }

    /// The library folder (canonical).
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_read_only(&self) -> bool {
        self.writer.is_none()
    }

    /// A path inside the library from a library-relative one (see [`resolve_inside`]).
    pub fn resolve(&self, rel: &Path) -> Result<PathBuf, AppError> {
        resolve_inside(&self.root, rel)
    }

    /// Runs `read` on the reader connection, which may run while the writer is busy.
    pub fn read<T>(
        &self,
        read: impl FnOnce(&Catalog) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        read(&lock(&self.reader))
    }

    /// Runs `write` on the writer connection; refused for a read-only library.
    pub fn write<T>(
        &self,
        write: impl FnOnce(&mut Catalog) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let Some(writer) = &self.writer else {
            return Err(AppError::new(
                ErrorCode::LibraryTooNew,
                "This library is read-only because a newer version of Grain to Pixel made it.",
            ));
        };
        write(&mut lock(writer))
    }

    pub fn details(&self) -> Result<LibraryDetails, AppError> {
        let (name, created_at, schema_version) = self.read(|catalog| {
            Ok((
                catalog.meta_get(NAME_KEY)?,
                catalog.meta_get(CREATED_KEY)?,
                catalog.schema_version()?,
            ))
        })?;
        let folder_name = self
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(LibraryDetails {
            path: display_path(&self.root),
            name: name.unwrap_or(folder_name),
            created_at,
            read_only: self.is_read_only(),
            schema_version,
            free_bytes: library_fs::free_space(&self.root).ok(),
        })
    }
}

#[cfg(test)]
mod tests {
    use gtp_catalog::SCHEMA_VERSION;
    use gtp_core::{FilmFormat, FrameId, Roll, RollId};

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            Self(std::env::temp_dir().join(format!(
                "gtp-library-{label}-{}-{}",
                std::process::id(),
                FrameId::new()
            )))
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn roll() -> Roll {
        Roll {
            id: RollId::new(),
            name: "Roll 1".into(),
            film_stock: None,
            format: FilmFormat::Mm35,
            camera: None,
            iso_box: None,
            iso_shot: None,
            shot_from: None,
            shot_to: None,
            notes: String::new(),
            film_base_rgb: None,
            film_base_confidence: None,
            created_at: String::new(),
        }
    }

    #[test]
    fn a_new_library_has_its_layout_and_reopens_with_its_data() {
        let dir = TempDir::new("create");
        let lib = Library::create(&dir.0, "  Negatives  ").unwrap();
        for sub in ["originals", "thumbs", "exports", "library.db"] {
            assert!(dir.0.join(sub).exists(), "{sub}");
        }
        let details = lib.details().unwrap();
        assert_eq!(details.name, "Negatives");
        assert!(!details.read_only);
        assert_eq!(details.schema_version, SCHEMA_VERSION);
        assert!(details.created_at.is_some_and(|t| t.ends_with('Z')));
        assert!(details.free_bytes.is_some_and(|b| b > 0));
        assert!(!details.path.starts_with(r"\\?\"));
        let stored = lib.write(|c| c.insert_roll(&roll())).unwrap();
        // The reader sees the writer's commit.
        assert_eq!(lib.read(|c| c.list_rolls()).unwrap(), vec![stored.clone()]);
        drop(lib);

        std::fs::remove_dir_all(dir.0.join("thumbs")).unwrap();
        let lib = Library::open(&dir.0).unwrap();
        assert!(
            dir.0.join("thumbs").is_dir(),
            "a missing cache folder is recreated"
        );
        assert_eq!(lib.read(|c| c.list_rolls()).unwrap(), vec![stored]);
    }

    #[test]
    fn create_refuses_a_non_empty_folder_and_an_existing_library() {
        let dir = TempDir::new("refuse");
        std::fs::create_dir_all(&dir.0).unwrap();
        std::fs::write(dir.0.join("holiday.jpg"), b"x").unwrap();
        let err = Library::create(&dir.0, "Lib").err().unwrap();
        assert_eq!(err.code, ErrorCode::InvalidInput);

        let lib_dir = TempDir::new("existing");
        drop(Library::create(&lib_dir.0, "Lib").unwrap());
        let err = Library::create(&lib_dir.0, "Again").err().unwrap();
        assert!(err.message.contains("Open it instead"));
        assert_eq!(
            Library::create(&TempDir::new("noname").0, "   ")
                .err()
                .unwrap()
                .code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn a_folder_without_a_library_is_not_found() {
        let dir = TempDir::new("plain");
        std::fs::create_dir_all(&dir.0).unwrap();
        assert_eq!(
            Library::open(&dir.0).err().unwrap().code,
            ErrorCode::NotFound
        );
        let missing = TempDir::new("missing");
        assert_eq!(
            Library::open(&missing.0).err().unwrap().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_newer_library_opens_read_only() {
        let dir = TempDir::new("newer");
        let lib = Library::create(&dir.0, "Future").unwrap();
        let stored = lib.write(|c| c.insert_roll(&roll())).unwrap();
        drop(lib);
        let conn = rusqlite_set_version(&dir.0.join(DB_FILE), SCHEMA_VERSION + 1);
        drop(conn);

        let lib = Library::open(&dir.0).unwrap();
        assert!(lib.is_read_only());
        assert!(lib.details().unwrap().read_only);
        assert_eq!(lib.read(|c| c.list_rolls()).unwrap(), vec![stored]);
        let err = lib.write(|c| c.insert_roll(&roll())).unwrap_err();
        assert_eq!(err.code, ErrorCode::LibraryTooNew);
        assert!(err.message.contains("read-only"));
    }

    #[test]
    fn library_paths_stay_inside_the_library() {
        let dir = TempDir::new("resolve");
        let lib = Library::create(&dir.0, "Lib").unwrap();
        assert!(lib.resolve(Path::new("thumbs/a.jpg")).is_ok());
        assert!(lib.resolve(Path::new("../a.jpg")).is_err());
    }

    /// Sets the schema version as a newer app would (test helper using the catalog's own reader
    /// path is not possible: it refuses writes).
    fn rusqlite_set_version(db: &Path, version: u32) -> std::fs::File {
        // SQLite stores user_version as a big-endian u32 at byte 60 of the header.
        use std::io::{Seek, SeekFrom, Write};
        let mut file = std::fs::OpenOptions::new().write(true).open(db).unwrap();
        file.seek(SeekFrom::Start(60)).unwrap();
        file.write_all(&version.to_be_bytes()).unwrap();
        file
    }
}
