//! The capture settings the sequence reads (docs/specification.md section 6.1, ADR 0004
//! amendments 4 and 5). Saving them in `settings.json` comes with D-04; this is the plain struct.

use std::path::PathBuf;

/// A supply voltage below this many millivolts warns that the supply is failing (the author's
/// light sagged from 5160 to 5086 mV at full power; ADR 0004 amendment 4).
pub const VBUS_WARN_MV: i32 = 4750;

/// How the pictures reach the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraBackend {
    /// libgphoto2 over USB (macOS).
    Gphoto,
    /// A tool such as digiCamControl writes the NEF files to a folder.
    Folder,
}

/// What a capture sequence is set up with.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureProfile {
    /// The red, green and blue LED level (0 to 255) for one exposure of that colour: the three
    /// LEDs need very different levels for one camera exposure. The defaults (44, 22, 2) are
    /// starting values measured on the bare light with no film in the gate (the blue was still
    /// clipped at 2); they are not balanced for film and must be rebalanced with a test shot.
    pub levels: [u8; 3],
    /// Wait between switching the colour on and firing the camera.
    pub settle_ms: u32,
    /// Above this LED temperature (degrees Celsius) the sequence warns and goes on.
    pub temp_warn_c: f32,
    /// Above this LED temperature the sequence stops.
    pub temp_abort_c: f32,
    /// Where the pictures come from.
    pub camera_backend: CameraBackend,
    /// The folder the pictures appear in, for [`CameraBackend::Folder`].
    pub watch_folder: Option<PathBuf>,
}

impl Default for CaptureProfile {
    fn default() -> Self {
        Self {
            levels: [44, 22, 2],
            settle_ms: 500,
            temp_warn_c: 45.0,
            temp_abort_c: 55.0,
            camera_backend: if cfg!(target_os = "macos") {
                CameraBackend::Gphoto
            } else {
                CameraBackend::Folder
            },
            watch_folder: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_bare_light_starting_values_the_spikes_measured() {
        let profile = CaptureProfile::default();
        assert_eq!(profile.levels, [44, 22, 2]);
        assert_eq!(profile.settle_ms, 500);
        assert_eq!((profile.temp_warn_c, profile.temp_abort_c), (45.0, 55.0));
        assert_eq!(VBUS_WARN_MV, 4750);
    }
}
