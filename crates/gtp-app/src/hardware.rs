//! The devices the app talks to, chosen once at start. With `GTP_FAKE_HW=1` they are fixtures, so
//! the app and its scan jobs run without hardware; otherwise each device is real, and one that has
//! no adapter yet is simply absent.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gtp_ingest::{FixtureSource, ScannerSource};

/// The environment variable that switches the app to fixture devices.
pub const FAKE_HW_VAR: &str = "GTP_FAKE_HW";
/// The folder of TIFFs the fixture scanner replays; defaults to `samples/private` of the repository
/// this build was made in.
pub const FIXTURE_DIR_VAR: &str = "GTP_FIXTURE_DIR";

/// The scanner (and, with the capture issues, the light and camera) the app uses.
pub struct Hardware {
    /// `None` until a real scanner adapter exists and fixtures are off.
    pub scanner: Option<Arc<dyn ScannerSource>>,
    fixture_dir: Option<PathBuf>,
}

impl Hardware {
    /// Chooses the devices from the environment (`GTP_FAKE_HW`, `GTP_FIXTURE_DIR`).
    pub fn from_env() -> Self {
        Self::from_vars(
            std::env::var(FAKE_HW_VAR).ok().as_deref(),
            std::env::var_os(FIXTURE_DIR_VAR),
        )
    }

    /// [`Hardware::from_env`] with the two values given: fixtures when `fake` is `1`.
    pub fn from_vars(fake: Option<&str>, fixture_dir: Option<OsString>) -> Self {
        if fake.map(str::trim) != Some("1") {
            return Self {
                scanner: None,
                fixture_dir: None,
            };
        }
        let dir = fixture_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/private"));
        Self {
            scanner: Some(Arc::new(FixtureSource::new(&dir))),
            fixture_dir: Some(dir),
        }
    }

    /// True when the devices are fixtures.
    pub fn is_fake(&self) -> bool {
        self.fixture_dir.is_some()
    }

    /// The folder the fixture scanner replays; `None` unless the devices are fixtures.
    pub fn fixture_dir(&self) -> Option<&Path> {
        self.fixture_dir.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use gtp_core::ErrorCode;

    use super::*;

    #[test]
    fn only_the_value_1_switches_to_fixtures() {
        for off in [
            None,
            Some(""),
            Some("0"),
            Some("true"),
            Some("yes"),
            Some("11"),
        ] {
            let hw = Hardware::from_vars(off, None);
            assert!(!hw.is_fake(), "{off:?}");
            assert!(hw.scanner.is_none() && hw.fixture_dir().is_none());
        }
        for on in [Some("1"), Some(" 1 ")] {
            let hw = Hardware::from_vars(on, None);
            assert!(hw.is_fake(), "{on:?}");
            assert!(hw.scanner.is_some());
        }
    }

    #[test]
    fn the_fixture_folder_is_the_variable_or_the_private_samples() {
        let chosen = Hardware::from_vars(Some("1"), Some("some/folder".into()));
        assert_eq!(chosen.fixture_dir(), Some(Path::new("some/folder")));
        let default = Hardware::from_vars(Some("1"), None);
        let dir = default.fixture_dir().unwrap();
        assert!(dir.ends_with("../../samples/private"), "{}", dir.display());
    }

    #[test]
    fn the_chosen_scanner_replays_the_folder_through_the_port() {
        let missing = std::env::temp_dir().join(format!("gtp-no-fixtures-{}", std::process::id()));
        let hw = Hardware::from_vars(Some("1"), Some(missing.into()));
        let scanner = hw.scanner.as_ref().unwrap();
        assert_eq!(
            scanner.list_devices().unwrap_err().code,
            ErrorCode::ScannerUnavailable,
            "a missing folder means no scanner, not a crash"
        );
    }
}
