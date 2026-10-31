//! The library folder layout (docs/specification.md section 5.1):
//! `<Library>/{library.db, originals/<roll_id>/<frame_id>/, thumbs/<frame_id>.jpg, exports/}`.

use std::path::{Path, PathBuf};

use gtp_core::{AppError, FrameId, RollId};

pub const DB_FILE: &str = "library.db";
pub const ORIGINALS_DIR: &str = "originals";
pub const THUMBS_DIR: &str = "thumbs";
pub const EXPORTS_DIR: &str = "exports";
const DIRS: [&str; 3] = [ORIGINALS_DIR, THUMBS_DIR, EXPORTS_DIR];

/// True when `root` holds a library database.
pub fn is_library(root: &Path) -> bool {
    root.join(DB_FILE).is_file()
}

/// Creates the folders of the layout that are missing (the database is the catalog's job).
pub fn ensure_layout(root: &Path) -> Result<(), AppError> {
    for dir in DIRS {
        std::fs::create_dir_all(root.join(dir)).map_err(|err| AppError::from_io(&err))?;
    }
    Ok(())
}

/// The folder of a frame's originals, relative to the library root.
pub fn originals_rel(roll: RollId, frame: FrameId) -> PathBuf {
    Path::new(ORIGINALS_DIR)
        .join(roll.to_string())
        .join(frame.to_string())
}

/// A frame's thumbnail, relative to the library root.
pub fn thumb_rel(frame: FrameId) -> PathBuf {
    Path::new(THUMBS_DIR).join(format!("{frame}.jpg"))
}

/// Bytes available to the user on the drive holding `path`.
pub fn free_space(path: &Path) -> Result<u64, AppError> {
    fs4::available_space(path).map_err(|err| AppError::from_io(&err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_is_created_and_relative_paths_follow_it() {
        let root = std::env::temp_dir().join(format!(
            "gtp-layout-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        ensure_layout(&root).unwrap();
        ensure_layout(&root).unwrap();
        for dir in DIRS {
            assert!(root.join(dir).is_dir());
        }
        assert!(!is_library(&root));
        assert!(free_space(&root).unwrap() > 0);

        let (roll, frame) = (RollId::new(), FrameId::new());
        assert_eq!(
            originals_rel(roll, frame),
            Path::new("originals")
                .join(roll.to_string())
                .join(frame.to_string())
        );
        assert_eq!(
            thumb_rel(frame),
            Path::new("thumbs").join(format!("{frame}.jpg"))
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
