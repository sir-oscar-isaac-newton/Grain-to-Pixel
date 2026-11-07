//! The capture settings the sequence reads (docs/specification.md section 6.1, ADR 0004
//! amendments 4 and 5), their validation and the patch the UI sends. The application layer keeps
//! the profile in `settings.json`.

use std::path::PathBuf;

use gtp_core::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};

/// A supply voltage below this many millivolts warns that the supply is failing (the author's
/// light sagged from 5160 to 5086 mV at full power; ADR 0004 amendment 4).
pub const VBUS_WARN_MV: i32 = 4750;

/// The longest settle time accepted, in milliseconds.
pub const MAX_SETTLE_MS: u32 = 10_000;

/// The longest watch folder path accepted, in characters.
const MAX_FOLDER_CHARS: usize = 1024;

/// How the pictures reach the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum CameraBackend {
    /// libgphoto2 over USB (macOS).
    Gphoto,
    /// A tool such as digiCamControl writes the NEF files to a folder.
    Folder,
}

/// What a capture sequence is set up with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
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
    /// Above this LED temperature the sequence stops, and the light is switched off.
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

/// Profile fields to change; the ones left out keep their value.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct CaptureProfilePatch {
    /// New red, green and blue levels. Wider than a byte so that a value outside 0 to 255 reaches
    /// the validation and is refused with a message, not as a malformed request.
    pub levels: Option<[u32; 3]>,
    pub settle_ms: Option<u32>,
    pub temp_warn_c: Option<f32>,
    pub temp_abort_c: Option<f32>,
    pub camera_backend: Option<CameraBackend>,
    /// The watch folder; an empty text clears it.
    pub watch_folder: Option<String>,
}

fn invalid(message: &str, details: String) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message).with_details(details)
}

impl CaptureProfile {
    /// Checks the profile: the temperatures are finite numbers with the warn limit below the abort
    /// limit, the settle time is at most [`MAX_SETTLE_MS`], and the watch folder is a plain path.
    /// (The levels are bytes, so they are in 0 to 255 by type; a patch is checked in
    /// [`CaptureProfile::patched`].)
    pub fn validate(&self) -> Result<(), AppError> {
        if !self.temp_warn_c.is_finite() || !self.temp_abort_c.is_finite() {
            return Err(invalid(
                "The temperature limits must be numbers.",
                format!("warn {}, abort {}", self.temp_warn_c, self.temp_abort_c),
            ));
        }
        if self.temp_warn_c >= self.temp_abort_c {
            return Err(invalid(
                "The warning temperature must be lower than the temperature at which the light is switched off.",
                format!("warn {}, abort {}", self.temp_warn_c, self.temp_abort_c),
            ));
        }
        if self.settle_ms > MAX_SETTLE_MS {
            return Err(invalid(
                "The settle time is too long.",
                format!("{} ms of at most {MAX_SETTLE_MS}", self.settle_ms),
            ));
        }
        if let Some(folder) = &self.watch_folder {
            let text = folder.to_string_lossy();
            if text.trim().is_empty()
                || text.chars().count() > MAX_FOLDER_CHARS
                || text.chars().any(char::is_control)
            {
                return Err(invalid(
                    "The watch folder is not a valid path.",
                    format!("{} characters", text.chars().count()),
                ));
            }
        }
        Ok(())
    }

    /// This profile with `patch` applied, validated as a whole. Nothing changes when it is refused.
    pub fn patched(&self, patch: &CaptureProfilePatch) -> Result<Self, AppError> {
        let mut next = self.clone();
        if let Some(levels) = patch.levels {
            let mut bytes = [0u8; 3];
            for (slot, level) in bytes.iter_mut().zip(levels) {
                *slot = u8::try_from(level).map_err(|_| {
                    invalid(
                        "A light level is 0 to 255.",
                        format!("levels {}, {}, {}", levels[0], levels[1], levels[2]),
                    )
                })?;
            }
            next.levels = bytes;
        }
        if let Some(settle_ms) = patch.settle_ms {
            next.settle_ms = settle_ms;
        }
        if let Some(warn) = patch.temp_warn_c {
            next.temp_warn_c = warn;
        }
        if let Some(abort) = patch.temp_abort_c {
            next.temp_abort_c = abort;
        }
        if let Some(backend) = patch.camera_backend {
            next.camera_backend = backend;
        }
        if let Some(folder) = &patch.watch_folder {
            let folder = folder.trim();
            next.watch_folder = (!folder.is_empty()).then(|| PathBuf::from(folder));
        }
        next.validate()?;
        Ok(next)
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
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn a_patch_changes_only_its_fields() {
        let base = CaptureProfile::default();
        let patch = CaptureProfilePatch {
            levels: Some([255, 0, 7]),
            ..CaptureProfilePatch::default()
        };
        let next = base.patched(&patch).unwrap();
        assert_eq!(next.levels, [255, 0, 7]);
        assert_eq!(
            CaptureProfile {
                levels: base.levels,
                ..next
            },
            base
        );
        assert_eq!(base.patched(&CaptureProfilePatch::default()).unwrap(), base);
    }

    #[test]
    fn a_level_above_255_is_refused() {
        let base = CaptureProfile::default();
        for levels in [[256, 0, 0], [0, 1000, 0], [0, 0, u32::MAX]] {
            let err = base
                .patched(&CaptureProfilePatch {
                    levels: Some(levels),
                    settle_ms: Some(900),
                    ..CaptureProfilePatch::default()
                })
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{levels:?}");
        }
        assert!(base
            .patched(&CaptureProfilePatch {
                levels: Some([0, 0, 255]),
                ..CaptureProfilePatch::default()
            })
            .is_ok());
    }

    #[test]
    fn the_warn_limit_must_be_below_the_abort_limit() {
        let base = CaptureProfile::default();
        for (warn, abort) in [(55.0, 55.0), (60.0, 55.0)] {
            let patch = CaptureProfilePatch {
                temp_warn_c: Some(warn),
                temp_abort_c: Some(abort),
                ..CaptureProfilePatch::default()
            };
            assert_eq!(
                base.patched(&patch).unwrap_err().code,
                ErrorCode::InvalidInput
            );
        }
        // Each side alone is checked against the other one as stored.
        let lower_abort = CaptureProfilePatch {
            temp_abort_c: Some(44.0),
            ..CaptureProfilePatch::default()
        };
        assert!(base.patched(&lower_abort).is_err());
        let higher_warn = CaptureProfilePatch {
            temp_warn_c: Some(56.0),
            ..CaptureProfilePatch::default()
        };
        assert!(base.patched(&higher_warn).is_err());
        let both = CaptureProfilePatch {
            temp_warn_c: Some(30.0),
            temp_abort_c: Some(40.0),
            ..CaptureProfilePatch::default()
        };
        assert_eq!(base.patched(&both).unwrap().temp_abort_c, 40.0);
    }

    #[test]
    fn non_finite_temperatures_are_refused() {
        let base = CaptureProfile::default();
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for patch in [
                CaptureProfilePatch {
                    temp_warn_c: Some(value),
                    ..CaptureProfilePatch::default()
                },
                CaptureProfilePatch {
                    temp_abort_c: Some(value),
                    ..CaptureProfilePatch::default()
                },
            ] {
                assert!(base.patched(&patch).is_err(), "{value}");
            }
        }
    }

    #[test]
    fn the_settle_time_and_the_watch_folder_are_checked_and_the_folder_can_be_cleared() {
        let base = CaptureProfile::default();
        let long = CaptureProfilePatch {
            settle_ms: Some(MAX_SETTLE_MS + 1),
            ..CaptureProfilePatch::default()
        };
        assert!(base.patched(&long).is_err());
        let set = |folder: &str| {
            base.patched(&CaptureProfilePatch {
                watch_folder: Some(folder.to_owned()),
                ..CaptureProfilePatch::default()
            })
        };
        let with_folder = set("  D:/Shots  ").unwrap();
        assert_eq!(with_folder.watch_folder, Some(PathBuf::from("D:/Shots")));
        let cleared = with_folder
            .patched(&CaptureProfilePatch {
                watch_folder: Some(String::new()),
                ..CaptureProfilePatch::default()
            })
            .unwrap();
        assert_eq!(cleared.watch_folder, None);
        assert!(set("D:/a\nb").is_err());
        assert!(set(&"x".repeat(2000)).is_err());
    }

    #[test]
    fn a_profile_survives_json_and_a_missing_field_takes_its_default() {
        let profile = CaptureProfile {
            levels: [1, 2, 3],
            camera_backend: CameraBackend::Gphoto,
            watch_folder: Some(PathBuf::from("D:/Shots")),
            ..CaptureProfile::default()
        };
        let text = serde_json::to_string(&profile).unwrap();
        let back: CaptureProfile = serde_json::from_str(&text).unwrap();
        assert_eq!(back, profile);
        let partial: CaptureProfile = serde_json::from_str(r#"{"settle_ms": 900}"#).unwrap();
        assert_eq!(
            partial,
            CaptureProfile {
                settle_ms: 900,
                ..CaptureProfile::default()
            }
        );
    }
}
