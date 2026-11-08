//! The camera's exposure settings (H-16): ISO, shutter speed and aperture, read from the camera
//! with the choices it allows, and changed one at a time. Plain types and checks that build and are
//! tested on every platform; the libgphoto2 calls are in `gphoto/session.rs`.
//!
//! The camera's own dials are locked while it is tethered, so this is the way to change them. What
//! the camera reports is checked at the boundary: choices are bounded in number and length and
//! stripped of control characters, because they come from a device.

use gtp_core::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};

/// The most choices kept for one setting (a D600 lists about 50 shutter speeds).
pub const MAX_CHOICES: usize = 200;
/// The longest choice or value, in characters; anything longer is not a camera setting.
pub const MAX_VALUE_CHARS: usize = 32;

/// One of the settings the capture view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum CameraSettingKind {
    /// The sensor sensitivity.
    Iso,
    /// The exposure time.
    ShutterSpeed,
    /// The lens aperture (f-number).
    Aperture,
}

impl CameraSettingKind {
    /// Every setting, in the order the view shows them.
    pub const ALL: [Self; 3] = [Self::Iso, Self::ShutterSpeed, Self::Aperture];

    /// The libgphoto2 configuration key of the setting (the single place the keys are named).
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Iso => "iso",
            Self::ShutterSpeed => "shutterspeed",
            Self::Aperture => "f-number",
        }
    }
}

/// A setting as the camera has it now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CameraSetting {
    /// Which setting this is.
    pub kind: CameraSettingKind,
    /// The current value as the camera words it (`"100"`, `"1/100"`, `"8"`); `None` when the
    /// camera has no such setting.
    pub value: Option<String>,
    /// The values the camera allows now, in its order. Empty when the setting is absent.
    pub choices: Vec<String>,
    /// False when the camera has no such setting or does not let it be changed now (the shutter
    /// speed and aperture only change in M mode, and a lens without electronic contacts has no
    /// aperture to set).
    pub writable: bool,
}

impl CameraSetting {
    /// A setting the camera does not have (unknown key, not supported, or not a menu).
    #[must_use]
    pub fn absent(kind: CameraSettingKind) -> Self {
        Self {
            kind,
            value: None,
            choices: Vec::new(),
            writable: false,
        }
    }

    /// A setting read from the camera: the text is cleaned and bounded first (see [`clean_text`]).
    #[must_use]
    pub fn from_camera(
        kind: CameraSettingKind,
        value: &str,
        choices: impl IntoIterator<Item = String>,
        writable: bool,
    ) -> Self {
        let choices: Vec<String> = choices
            .into_iter()
            .filter_map(|choice| clean_text(&choice))
            .take(MAX_CHOICES)
            .collect();
        Self {
            kind,
            value: clean_text(value),
            choices,
            writable,
        }
    }
}

/// `text` trimmed and without control characters, or `None` when nothing is left or it is longer
/// than [`MAX_VALUE_CHARS`] (never cut short: a cut choice could not be set again).
#[must_use]
pub fn clean_text(text: &str) -> Option<String> {
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    if clean.is_empty() || clean.chars().count() > MAX_VALUE_CHARS {
        None
    } else {
        Some(clean.to_owned())
    }
}

/// A value the user wants to set, trimmed and checked for length and control characters.
pub fn clean_value(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > MAX_VALUE_CHARS
        || value.chars().any(char::is_control)
    {
        return Err(not_offered());
    }
    Ok(value.to_owned())
}

/// Checks a change against the settings as the camera has them: the setting must exist and be
/// writable now, and `value` must be one of its choices.
pub fn check_change(
    current: &[CameraSetting],
    kind: CameraSettingKind,
    value: &str,
) -> Result<(), AppError> {
    let Some(setting) = current.iter().find(|s| s.kind == kind) else {
        return Err(absent_error());
    };
    if setting.choices.is_empty() {
        return Err(absent_error());
    }
    if !setting.writable {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The camera does not let this setting be changed now. Is it in M mode, and does the lens have electronic contacts?",
        ));
    }
    if !setting.choices.iter().any(|choice| choice == value) {
        return Err(not_offered());
    }
    Ok(())
}

/// True when a failure of reading a configuration key means the camera simply does not have that
/// setting, as opposed to a broken link: libgphoto2 says "bad parameters" or "not supported" for a
/// key the driver lacks, and the binding says "Expected RadioWidget but got ..." for a key that is
/// not a menu. `unsupported` is whether libgphoto2's code was one of the first two.
#[must_use]
pub fn is_missing_setting(unsupported: bool, text: &str) -> bool {
    unsupported || text.contains("Expected RadioWidget")
}

fn not_offered() -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "That value is not one the camera offers.",
    )
}

fn absent_error() -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "The camera does not have this setting.",
    )
}

/// Reads and changes the exposure settings of a camera. Kept apart from `CameraSource` for the
/// reason `CameraFinder` is: a job holds the source for its whole run, while the UI asks for the
/// settings between jobs. Callers refuse changes while a capture runs; a backend also refuses while
/// a picture is being taken.
pub trait CameraSettings: Send + Sync {
    /// The three settings, in [`CameraSettingKind::ALL`] order; one the camera lacks is
    /// [`CameraSetting::absent`].
    fn settings(&self) -> Result<Vec<CameraSetting>, AppError>;
    /// Sets one setting to one of its choices and returns the settings as the camera has them
    /// afterwards (a change of the shutter speed can alter what the aperture allows).
    fn set_setting(
        &self,
        kind: CameraSettingKind,
        value: &str,
    ) -> Result<Vec<CameraSetting>, AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(kind: CameraSettingKind, writable: bool) -> CameraSetting {
        CameraSetting::from_camera(kind, "100", ["100".to_owned(), "200".to_owned()], writable)
    }

    #[test]
    fn each_kind_has_its_libgphoto2_key_and_its_ui_name() {
        assert_eq!(CameraSettingKind::Iso.key(), "iso");
        assert_eq!(CameraSettingKind::ShutterSpeed.key(), "shutterspeed");
        assert_eq!(CameraSettingKind::Aperture.key(), "f-number");
        let names: Vec<String> = CameraSettingKind::ALL
            .iter()
            .map(|k| serde_json::to_string(k).unwrap())
            .collect();
        assert_eq!(names, ["\"iso\"", "\"shutter_speed\"", "\"aperture\""]);
        let back: CameraSettingKind = serde_json::from_str("\"shutter_speed\"").unwrap();
        assert_eq!(back, CameraSettingKind::ShutterSpeed);
    }

    #[test]
    fn what_the_camera_reports_is_cleaned_and_bounded() {
        let long = "x".repeat(MAX_VALUE_CHARS + 1);
        let many = (0..MAX_CHOICES + 50).map(|n| n.to_string());
        let s = CameraSetting::from_camera(CameraSettingKind::Iso, " 4\u{7}00 ", many, true);
        assert_eq!(s.value.as_deref(), Some("400"));
        assert_eq!(s.choices.len(), MAX_CHOICES);

        let s = CameraSetting::from_camera(
            CameraSettingKind::Aperture,
            &long,
            [
                "".to_owned(),
                "  ".to_owned(),
                long.clone(),
                " 5.6\n".to_owned(),
            ],
            true,
        );
        assert_eq!(s.value, None, "an overlong value is dropped, not cut");
        assert_eq!(s.choices, ["5.6"]);
        assert_eq!(
            clean_text(&"y".repeat(MAX_VALUE_CHARS)).map(|t| t.len()),
            Some(32)
        );
    }

    #[test]
    fn an_absent_setting_has_no_value_choices_or_write_access() {
        let s = CameraSetting::absent(CameraSettingKind::Aperture);
        assert_eq!((s.value, s.choices, s.writable), (None, vec![], false));
    }

    #[test]
    fn a_key_the_camera_lacks_is_told_apart_from_a_broken_link() {
        assert!(is_missing_setting(true, "Bad parameters"));
        assert!(is_missing_setting(
            false,
            "Expected RadioWidget but got Text(..)"
        ));
        assert!(!is_missing_setting(false, "Input/output error"));
    }

    #[test]
    fn a_value_to_set_is_trimmed_and_bounded() {
        assert_eq!(clean_value("  1/100 ").unwrap(), "1/100");
        for bad in ["", "   ", "1\u{0}0", &"9".repeat(MAX_VALUE_CHARS + 1)] {
            let err = clean_value(bad).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
            assert!(err.message.contains("not one the camera offers"));
        }
    }

    #[test]
    fn a_change_must_be_a_choice_of_a_writable_setting_that_exists() {
        let current = [
            setting(CameraSettingKind::Iso, true),
            setting(CameraSettingKind::ShutterSpeed, false),
            CameraSetting::absent(CameraSettingKind::Aperture),
        ];
        assert!(check_change(&current, CameraSettingKind::Iso, "200").is_ok());
        let not_offered = check_change(&current, CameraSettingKind::Iso, "300").unwrap_err();
        assert!(not_offered.message.contains("not one the camera offers"));
        let readonly = check_change(&current, CameraSettingKind::ShutterSpeed, "100").unwrap_err();
        assert!(readonly.message.contains("M mode"), "{readonly}");
        let absent = check_change(&current, CameraSettingKind::Aperture, "8").unwrap_err();
        assert!(absent.message.contains("does not have"), "{absent}");
        assert!(check_change(&[], CameraSettingKind::Iso, "100").is_err());
    }
}
