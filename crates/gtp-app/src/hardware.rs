//! The devices the app talks to, chosen once at start. With `GTP_FAKE_HW=1` they are fixtures, so
//! the app and its scan jobs run without hardware; otherwise each device is real: the scanner is
//! the nkscan command line (`GTP_NKSCAN`, or `nkscan` on `PATH`), and a device that has no adapter
//! yet is simply absent.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gtp_capture::{list_ports, FakeLight, ScanlightV4};
use gtp_ingest::{FixtureSource, NkscanSource, ScannerSource};
use gtp_io::{RawDecoder, RawlerDecoder};

use crate::fake_capture::{fake_camera, fake_camera_info, LevelledRawDecoder};
use crate::services::capture_control::{BusyFlag, CameraSlot};
use crate::shared_light::{fake_ports, SharedLight};

/// The environment variable that switches the app to fixture devices.
pub const FAKE_HW_VAR: &str = "GTP_FAKE_HW";
/// The folder of TIFFs the fixture scanner replays; defaults to `samples/private` of the repository
/// this build was made in.
pub const FIXTURE_DIR_VAR: &str = "GTP_FIXTURE_DIR";

/// The scanner, the light and the camera the app uses.
pub struct Hardware {
    /// The nkscan scanner, or the fixture one with `GTP_FAKE_HW=1`. It is there even when no
    /// scanner is connected: listing the devices says so.
    pub scanner: Option<Arc<dyn ScannerSource>>,
    /// The Scanlight (not connected until `light_connect`), or the fake light with
    /// `GTP_FAKE_HW=1`. Always there: connecting says whether there is a light.
    pub light: SharedLight,
    /// The camera: the fake one with `GTP_FAKE_HW=1`, the libgphoto2 one on macOS builds with the
    /// `tether` feature (it may be plugged in later; see `CameraSlot`), otherwise none until the
    /// watched-folder backend (H-10).
    pub camera: Option<Arc<CameraSlot>>,
    /// Reads the camera's pictures: `rawler`, or the fake one with `GTP_FAKE_HW=1`.
    pub decoder: Arc<dyn RawDecoder>,
    busy: BusyFlag,
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
                scanner: Some(Arc::new(NkscanSource::from_env())),
                light: SharedLight::new(Box::new(ScanlightV4::new()), list_ports),
                camera: real_camera(),
                decoder: Arc::new(RawlerDecoder),
                busy: BusyFlag::default(),
                fixture_dir: None,
            };
        }
        let dir = fixture_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/private"));
        let fake_light = FakeLight::new();
        let decoder = LevelledRawDecoder::new(fake_light.handle());
        Self {
            scanner: Some(Arc::new(FixtureSource::new(&dir))),
            light: SharedLight::new(Box::new(fake_light), fake_ports),
            camera: Some(Arc::new({
                let camera = fake_camera();
                let settings = camera.handle();
                CameraSlot::new(fake_camera_info(), Box::new(camera))
                    .with_settings(Box::new(settings))
            })),
            decoder: Arc::new(decoder),
            busy: BusyFlag::default(),
            fixture_dir: Some(dir),
        }
    }

    /// These devices with another light, camera and decoder (tests that script them).
    #[must_use]
    pub fn with_capture(
        mut self,
        light: SharedLight,
        camera: Option<Arc<CameraSlot>>,
        decoder: Arc<dyn RawDecoder>,
    ) -> Self {
        self.light = light;
        self.camera = camera;
        self.decoder = decoder;
        self
    }

    /// The flag held while a capture or a test shot uses the light and the camera.
    pub fn busy(&self) -> &BusyFlag {
        &self.busy
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

/// The libgphoto2 camera: its worker starts now and the camera is found when it is needed.
#[cfg(all(feature = "tether", target_os = "macos"))]
fn real_camera() -> Option<Arc<CameraSlot>> {
    match gtp_capture::GphotoCamera::new() {
        Ok(camera) => {
            let finder = Box::new(camera.finder());
            let settings = Box::new(camera.finder());
            Some(Arc::new(
                CameraSlot::with_finder(Box::new(camera), finder).with_settings(settings),
            ))
        }
        Err(err) => {
            tracing::error!("the camera backend did not start: {err}");
            None
        }
    }
}

/// No camera backend in this build (not macOS, or the `tether` feature is off).
#[cfg(not(all(feature = "tether", target_os = "macos")))]
fn real_camera() -> Option<Arc<CameraSlot>> {
    None
}

/// Points libgphoto2 at the drivers inside the app bundle. Call it first thing in `main`, before
/// the app starts any thread (see `gtp_capture::configure_library_paths`). Does nothing without the
/// macOS `tether` feature. Returns whether the drivers of the bundle are used; it runs before
/// logging starts, so the caller logs this once the log is open ([`log_camera_libraries`]).
pub fn prepare_camera_libraries() -> bool {
    #[cfg(all(feature = "tether", target_os = "macos"))]
    {
        gtp_capture::configure_library_paths().is_some()
    }
    #[cfg(not(all(feature = "tether", target_os = "macos")))]
    {
        false
    }
}

/// Logs what [`prepare_camera_libraries`] did. Call it after logging has started.
pub fn log_camera_libraries(from_bundle: bool) {
    if from_bundle {
        tracing::info!("libgphoto2 drivers are taken from the app bundle");
    } else if cfg!(all(feature = "tether", target_os = "macos")) {
        tracing::info!("not running from an app bundle: libgphoto2 uses its own driver paths");
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
            assert!(hw.scanner.is_some() && hw.fixture_dir().is_none());
        }
        for on in [Some("1"), Some(" 1 ")] {
            let hw = Hardware::from_vars(on, None);
            assert!(hw.is_fake(), "{on:?}");
            assert!(hw.scanner.is_some());
        }
    }

    #[test]
    fn the_fake_devices_include_a_light_and_a_camera_and_the_real_ones_only_the_light() {
        let fake = Hardware::from_vars(Some("1"), None);
        assert_eq!(fake.light.list_ports().unwrap()[0].name, "fake");
        let camera = fake.camera.as_ref().map(|slot| slot.list());
        assert_eq!(camera.map(|l| l[0].id.clone()), Some("fake".to_owned()));
        let real = Hardware::from_vars(None, None);
        // The libgphoto2 camera exists only in macOS builds with the `tether` feature.
        assert_eq!(
            real.camera.is_some(),
            cfg!(all(feature = "tether", target_os = "macos"))
        );
        assert!(!real.busy().is_busy());
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
