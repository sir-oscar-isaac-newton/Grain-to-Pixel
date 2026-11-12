//! Test helper: a file in a folder of its own that is removed with the guard.

use std::path::{Path, PathBuf};

use gtp_core::testing::TempDir;

/// The path of a file (not yet written) in a unique folder that goes when the guard is dropped,
/// also when the test fails by panic.
pub(crate) struct TempFile {
    path: PathBuf,
    _dir: TempDir,
}

impl TempFile {
    /// A path `<unique folder>/<name>`; the file is for the test to create.
    pub(crate) fn new(label: &str, name: &str) -> Self {
        let dir = TempDir::new(label);
        Self {
            path: dir.path().join(name),
            _dir: dir,
        }
    }
}

impl std::ops::Deref for TempFile {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TempFile {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}
