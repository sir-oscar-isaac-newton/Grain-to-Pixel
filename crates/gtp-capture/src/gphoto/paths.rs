//! Where libgphoto2 finds its drivers. It reads `CAMLIBS` (camera drivers) and `IOLIBS` (port
//! drivers) when its first context lists or opens cameras, so a bundled app sets both to the
//! folders inside the bundle (`Contents/Frameworks/gphoto/{camlibs,iolibs}`, laid out by
//! `tools/scripts/release/bundle-libgphoto2.sh`). A development build has no such folders and
//! leaves the variables alone: libgphoto2 then uses the paths compiled into the Homebrew library.

use std::path::{Path, PathBuf};
use std::sync::Once;

/// The variable that names the camera driver folder.
pub const CAMLIBS_VAR: &str = "CAMLIBS";
/// The variable that names the port driver folder.
pub const IOLIBS_VAR: &str = "IOLIBS";

/// The driver folders of a bundled app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryDirs {
    /// Holds `ptp2.so`, the camera driver of every PTP camera, the D600 included.
    pub camlibs: PathBuf,
    /// Holds `usb1.so`, the USB port driver.
    pub iolibs: PathBuf,
}

/// The driver folders of the app bundle that `exe` (the app's binary, `Contents/MacOS/<name>`)
/// belongs to, or `None` when `exe` is not inside a bundle that has them. `is_dir` says whether a
/// folder exists (the real check is [`Path::is_dir`]).
pub fn bundled_library_dirs(exe: &Path, is_dir: impl Fn(&Path) -> bool) -> Option<LibraryDirs> {
    let contents = exe.parent()?.parent()?;
    let root = contents.join("Frameworks").join("gphoto");
    let dirs = LibraryDirs {
        camlibs: root.join("camlibs"),
        iolibs: root.join("iolibs"),
    };
    (is_dir(&dirs.camlibs) && is_dir(&dirs.iolibs)).then_some(dirs)
}

/// Points libgphoto2 at the drivers inside the app bundle, when this binary runs from one. Call it
/// once, first thing in `main`, before the app starts any thread and before any camera code: setting
/// an environment variable while another thread reads the environment is a data race in the C
/// library, and libgphoto2 reads these two on its own threads. Later calls do nothing. Returns the
/// folders it set, or `None` (a development build, nothing changed).
pub fn configure_library_paths() -> Option<LibraryDirs> {
    static ONCE: Once = Once::new();
    let mut set = None;
    ONCE.call_once(|| {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        set = bundled_library_dirs(&exe, Path::is_dir);
        if let Some(dirs) = &set {
            // Edition 2021: `set_var` is a safe function; the single-threaded start is the caller's
            // part of the contract above.
            std::env::set_var(CAMLIBS_VAR, &dirs.camlibs);
            std::env::set_var(IOLIBS_VAR, &dirs.iolibs);
            tracing::info!("libgphoto2 drivers are taken from the app bundle");
        } else {
            tracing::debug!("not running from an app bundle: libgphoto2 uses its own driver paths");
        }
    });
    set
}
