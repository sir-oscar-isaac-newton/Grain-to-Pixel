//! App-level settings (docs/specification.md sections 5.4 and 10): `settings.json` in the OS
//! app-config folder. Per-library values live in the catalog's `meta` table instead.
//!
//! A file that cannot be read never stops the app: it is kept as `settings.json.bad` and the
//! defaults are used. Saving writes a temporary file and renames it over the old one.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use gtp_core::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};

/// Window sizes accepted, in logical pixels.
pub const WINDOW_SIDE: std::ops::RangeInclusive<u32> = 200..=16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

/// Runtime feature flags with their defaults (spec section 10). Each flag is deleted after one
/// release in which it was fully on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Flags {
    pub dust_removal: bool,
    pub triplet_import: bool,
    pub guided_capture: bool,
    pub dev_overlay: bool,
    pub experimental_auto_levels: bool,
}

impl Default for Flags {
    fn default() -> Self {
        Self {
            dust_removal: true,
            triplet_import: true,
            guided_capture: true,
            dev_overlay: false,
            experimental_auto_levels: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Settings {
    /// The library opened last, reopened by the UI at start.
    pub last_library: Option<String>,
    pub window: Option<WindowSize>,
    pub theme: Theme,
    pub flags: Flags,
}

/// Flags to change; the others keep their value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct FlagsPatch {
    pub dust_removal: Option<bool>,
    pub triplet_import: Option<bool>,
    pub guided_capture: Option<bool>,
    pub dev_overlay: Option<bool>,
    pub experimental_auto_levels: Option<bool>,
}

/// Settings to change; fields left out keep their value. The last library is set by opening one.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct SettingsPatch {
    pub window: Option<WindowSize>,
    pub theme: Option<Theme>,
    pub flags: Option<FlagsPatch>,
}

impl Settings {
    fn apply(&mut self, patch: &SettingsPatch) -> Result<(), AppError> {
        if let Some(window) = patch.window {
            if !WINDOW_SIDE.contains(&window.width) || !WINDOW_SIDE.contains(&window.height) {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    "That window size is out of range.",
                )
                .with_details(format!("{}x{}", window.width, window.height)));
            }
            self.window = Some(window);
        }
        if let Some(theme) = patch.theme {
            self.theme = theme;
        }
        if let Some(f) = patch.flags {
            let flags = &mut self.flags;
            for (slot, value) in [
                (&mut flags.dust_removal, f.dust_removal),
                (&mut flags.triplet_import, f.triplet_import),
                (&mut flags.guided_capture, f.guided_capture),
                (&mut flags.dev_overlay, f.dev_overlay),
                (
                    &mut flags.experimental_auto_levels,
                    f.experimental_auto_levels,
                ),
            ] {
                if let Some(value) = value {
                    *slot = value;
                }
            }
        }
        Ok(())
    }
}

/// The settings file and the settings in memory.
pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<Settings>,
}

fn write_atomically(path: &Path, text: &str) -> Result<(), AppError> {
    let io = |err: std::io::Error| AppError::from_io(&err);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, text).map_err(io)?;
    std::fs::rename(&temp, path).map_err(|err| {
        let _ = std::fs::remove_file(&temp);
        io(err)
    })
}

impl SettingsStore {
    /// Loads `path`. Returns the store and, when the file was unreadable, a note for the log (the
    /// file is then kept as `<name>.bad` and the defaults are used).
    pub fn load(path: &Path) -> (Self, Option<String>) {
        let (settings, note) = match std::fs::read_to_string(path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => (Settings::default(), None),
            Err(err) => (
                Settings::default(),
                Some(format!("settings unreadable: {err}")),
            ),
            Ok(text) => match serde_json::from_str(&text) {
                Ok(settings) => (settings, None),
                Err(err) => {
                    let mut bad = path.as_os_str().to_owned();
                    bad.push(".bad");
                    let kept = std::fs::rename(path, PathBuf::from(&bad)).is_ok();
                    (
                        Settings::default(),
                        Some(format!(
                            "settings unreadable ({err}); using defaults{}",
                            if kept { ", old file kept as .bad" } else { "" }
                        )),
                    )
                }
            },
        };
        (
            Self {
                path: path.to_path_buf(),
                current: Mutex::new(settings),
            },
            note,
        )
    }

    pub fn get(&self) -> Settings {
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Applies `patch`, saves and returns the new settings. Nothing changes if the patch is invalid
    /// or the file cannot be written.
    pub fn update(&self, patch: &SettingsPatch) -> Result<Settings, AppError> {
        self.change(|settings| settings.apply(patch))
    }

    /// Records the library opened last.
    pub fn set_last_library(&self, path: &str) -> Result<Settings, AppError> {
        self.change(|settings| {
            settings.last_library = Some(path.to_owned());
            Ok(())
        })
    }

    fn change(
        &self,
        edit: impl FnOnce(&mut Settings) -> Result<(), AppError>,
    ) -> Result<Settings, AppError> {
        let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        let mut next = current.clone();
        edit(&mut next)?;
        let text = serde_json::to_string_pretty(&next).map_err(AppError::internal)?;
        write_atomically(&self.path, &text)?;
        *current = next.clone();
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(label: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "gtp-settings-{label}-{}-{}",
                std::process::id(),
                gtp_core::FrameId::new()
            ))
            .join("settings.json")
    }

    #[test]
    fn defaults_follow_the_spec() {
        let flags = Settings::default().flags;
        assert!(flags.dust_removal && flags.triplet_import && flags.guided_capture);
        assert!(!flags.dev_overlay && !flags.experimental_auto_levels);
        assert_eq!(Settings::default().theme, Theme::System);
    }

    #[test]
    fn a_patch_changes_only_its_fields_and_survives_a_reload() {
        let path = temp_file("roundtrip");
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        assert_eq!(store.get(), Settings::default());
        store.set_last_library("D:/Film/Library").unwrap();
        let patch = SettingsPatch {
            theme: Some(Theme::Dark),
            flags: Some(FlagsPatch {
                dev_overlay: Some(true),
                ..FlagsPatch::default()
            }),
            ..SettingsPatch::default()
        };
        let updated = store.update(&patch).unwrap();
        assert_eq!(updated.theme, Theme::Dark);
        assert!(updated.flags.dev_overlay && updated.flags.dust_removal);
        assert_eq!(updated.last_library.as_deref(), Some("D:/Film/Library"));

        let (reloaded, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        assert_eq!(reloaded.get(), updated);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_invalid_patch_changes_nothing() {
        let path = temp_file("invalid");
        let (store, _) = SettingsStore::load(&path);
        let patch = SettingsPatch {
            window: Some(WindowSize {
                width: 50,
                height: 800,
            }),
            theme: Some(Theme::Light),
            ..SettingsPatch::default()
        };
        assert_eq!(
            store.update(&patch).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        assert_eq!(store.get(), Settings::default());
        assert!(!path.exists());
    }

    #[test]
    fn a_corrupt_file_is_kept_aside_and_defaults_are_used() {
        let path = temp_file("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_some_and(|n| n.contains(".bad")));
        assert_eq!(store.get(), Settings::default());
        let mut bad = path.as_os_str().to_owned();
        bad.push(".bad");
        assert_eq!(
            std::fs::read_to_string(PathBuf::from(bad)).unwrap(),
            "{ not json"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn unknown_and_missing_fields_fall_back_to_defaults() {
        let path = temp_file("partial");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"theme":"light","flags":{"dev_overlay":true,"old_flag":true},"future":1}"#,
        )
        .unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        let s = store.get();
        assert_eq!(s.theme, Theme::Light);
        assert!(s.flags.dev_overlay && s.flags.dust_removal);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
