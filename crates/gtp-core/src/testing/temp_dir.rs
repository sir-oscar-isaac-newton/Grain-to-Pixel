//! A unique test folder that removes itself.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::FrameId;

/// A folder in the system temporary folder that no other test or run can share and that is
/// removed when the guard is dropped, also when the test fails by panic.
///
/// The name is the label, the process id, a counter of this process and a random id: process ids
/// are reused (Windows does it quickly), so a name made of the process id alone can meet a folder
/// an earlier run left behind. Removal is retried for up to two seconds and then given up without
/// an error. Close a database inside the folder first, since Windows will not delete an open file:
/// drop the guard after the thing that holds the database (last field of a struct, or bound before
/// it in a test).
pub struct TempDir(PathBuf);

static TEMP_DIRS: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    /// Creates the folder, named `gtp-<label>-<process id>-<count>-<random>`.
    ///
    /// # Panics
    /// When the folder cannot be created.
    #[must_use]
    // Test-only helper (the module is behind the `testing` feature): a test cannot go on without it.
    #[allow(clippy::unwrap_used)]
    pub fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gtp-{label}-{}-{}-{}",
            std::process::id(),
            TEMP_DIRS.fetch_add(1, Ordering::Relaxed),
            FrameId::new()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// The folder.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // A background thread (a thumbnail worker, say) may still hold a file for a moment after
        // the test is done, and Windows refuses to delete it: try again for a short while.
        for _ in 0..40 {
            if std::fs::remove_dir_all(&self.0).is_ok() || !self.0.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
