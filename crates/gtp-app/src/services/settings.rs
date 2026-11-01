//! App-level settings (docs/specification.md sections 5.4 and 10): `settings.json` in the OS
//! app-config folder. Per-library values live in the catalog's `meta` table instead.
//!
//! A file that cannot be read never stops the app: it is kept as `settings.json.bad` and the
//! defaults are used. Saving writes a temporary file and renames it over the old one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use gtp_core::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};

/// Window sizes accepted, in logical pixels.
pub const WINDOW_SIDE: std::ops::RangeInclusive<u32> = 200..=16_384;
/// Most key bindings kept, longest action name and longest key name, in characters.
pub const MAX_KEYS: usize = 64;
pub const MAX_ACTION_CHARS: usize = 64;
pub const MAX_KEY_CHARS: usize = 32;
/// The update check at launch runs at most this often, in seconds (specification section 15).
pub const UPDATE_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

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
    /// Ask GitHub Releases for a newer version at launch. The only network call the app makes.
    pub update_check: bool,
}

impl Default for Flags {
    fn default() -> Self {
        Self {
            dust_removal: true,
            triplet_import: true,
            guided_capture: true,
            dev_overlay: false,
            experimental_auto_levels: false,
            update_check: true,
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
    /// The keyboard shortcuts the user changed, action name to key (`Q`, `[`, `Escape`). An action
    /// that is not here has its default key, which the UI knows; the app only keeps the shape.
    pub keys: BTreeMap<String, String>,
    /// When the update check last got an answer, in seconds since 1970; none before the first.
    /// Kept by the app, not by a patch, and not shown to the UI.
    #[cfg_attr(feature = "specta", specta(skip))]
    pub last_update_check: Option<u64>,
}

impl Settings {
    /// Whether the update check at launch may ask now: the flag is on and the last answer is at
    /// least a day old (or in the future, which a clock set back can cause).
    pub fn update_check_due(&self, now: u64) -> bool {
        self.flags.update_check
            && self
                .last_update_check
                .is_none_or(|last| last > now || now - last >= UPDATE_CHECK_INTERVAL_SECS)
    }
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
    pub update_check: Option<bool>,
}

/// Settings to change; fields left out keep their value. The last library is set by opening one.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct SettingsPatch {
    pub window: Option<WindowSize>,
    pub theme: Option<Theme>,
    pub flags: Option<FlagsPatch>,
    /// Replaces all the key bindings (an empty map goes back to the defaults).
    pub keys: Option<BTreeMap<String, String>>,
}

/// Key bindings must be few, with names of a sensible length, and no two actions may share a key
/// (compared without case, since Shift does not change which action a key stands for).
fn check_keys(keys: &BTreeMap<String, String>) -> Result<(), AppError> {
    let invalid = |message: &str, details: String| {
        AppError::new(ErrorCode::InvalidInput, message).with_details(details)
    };
    if keys.len() > MAX_KEYS {
        return Err(invalid(
            "There are too many key bindings.",
            format!("{} of at most {MAX_KEYS}", keys.len()),
        ));
    }
    let mut used: BTreeMap<String, &str> = BTreeMap::new();
    for (action, key) in keys {
        let action_ok = !action.is_empty()
            && action.chars().count() <= MAX_ACTION_CHARS
            && action
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
        if !action_ok {
            return Err(invalid("That action name is not valid.", action.clone()));
        }
        let key_ok = !key.is_empty()
            && key.chars().count() <= MAX_KEY_CHARS
            && !key.chars().any(char::is_control);
        if !key_ok {
            return Err(invalid(
                "That key is not valid.",
                format!("{action}: {key:?}"),
            ));
        }
        if let Some(other) = used.insert(key.to_lowercase(), action) {
            return Err(invalid(
                "Two actions cannot share a key.",
                format!("{other} and {action}: {key:?}"),
            ));
        }
    }
    Ok(())
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
        if let Some(keys) = &patch.keys {
            check_keys(keys)?;
            self.keys = keys.clone();
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
                (&mut flags.update_check, f.update_check),
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

    /// Records when the update check last got an answer.
    pub fn set_last_update_check(&self, now: u64) -> Result<Settings, AppError> {
        self.change(|settings| {
            settings.last_update_check = Some(now);
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
        assert!(flags.update_check);
        assert_eq!(Settings::default().theme, Theme::System);
        assert_eq!(Settings::default().last_update_check, None);
    }

    #[test]
    fn the_update_check_is_due_once_a_day_and_never_when_off() {
        const DAY: u64 = UPDATE_CHECK_INTERVAL_SECS;
        let mut settings = Settings::default();
        assert!(settings.update_check_due(1_000), "never asked");
        settings.last_update_check = Some(1_000);
        assert!(!settings.update_check_due(1_000 + DAY - 1));
        assert!(settings.update_check_due(1_000 + DAY));
        assert!(settings.update_check_due(500), "a clock set back");
        settings.flags.update_check = false;
        assert!(!settings.update_check_due(1_000 + 10 * DAY));
        assert!(!Settings {
            flags: Flags {
                update_check: false,
                ..Flags::default()
            },
            ..Settings::default()
        }
        .update_check_due(0));
    }

    #[test]
    fn the_time_of_the_last_check_is_kept_across_a_reload() {
        let path = temp_file("last-check");
        let (store, _) = SettingsStore::load(&path);
        store.set_last_update_check(1_700_000_000).unwrap();
        let (reloaded, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        assert_eq!(reloaded.get().last_update_check, Some(1_700_000_000));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
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

    fn keys(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(action, key)| ((*action).to_owned(), (*key).to_owned()))
            .collect()
    }

    fn with_keys(map: BTreeMap<String, String>) -> SettingsPatch {
        SettingsPatch {
            keys: Some(map),
            ..SettingsPatch::default()
        }
    }

    #[test]
    fn there_are_no_changed_keys_until_the_user_changes_one() {
        assert!(Settings::default().keys.is_empty());
        // A file from before the key map has none either.
        let path = temp_file("no-keys");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"theme":"dark"}"#).unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        assert!(store.get().keys.is_empty());
    }

    #[test]
    fn key_bindings_are_saved_replaced_as_a_whole_and_reset_with_an_empty_map() {
        let path = temp_file("keys");
        let (store, _) = SettingsStore::load(&path);
        let first = keys(&[("colour.red.up", "r"), ("develop.fit", "0"), ("space", " ")]);
        let saved = store.update(&with_keys(first.clone())).unwrap();
        assert_eq!(saved.keys, first);
        // Another patch leaves them alone.
        let themed = store
            .update(&SettingsPatch {
                theme: Some(Theme::Light),
                ..SettingsPatch::default()
            })
            .unwrap();
        assert_eq!(themed.keys, first);
        // They survive a restart.
        let (again, _) = SettingsStore::load(&path);
        assert_eq!(again.get().keys, first);

        let second = keys(&[("develop.fit", "f")]);
        assert_eq!(
            store.update(&with_keys(second.clone())).unwrap().keys,
            second
        );
        assert!(store
            .update(&with_keys(BTreeMap::new()))
            .unwrap()
            .keys
            .is_empty());
    }

    #[test]
    fn invalid_key_bindings_are_refused_and_change_nothing() {
        let path = temp_file("bad-keys");
        let (store, _) = SettingsStore::load(&path);
        let kept = keys(&[("develop.fit", "f")]);
        store.update(&with_keys(kept.clone())).unwrap();

        let too_many: BTreeMap<String, String> = (0..=MAX_KEYS)
            .map(|i| {
                (
                    format!("action{i}"),
                    format!("{}", char::from_u32(0x100 + i as u32).unwrap()),
                )
            })
            .collect();
        let bad: Vec<BTreeMap<String, String>> = vec![
            too_many,
            keys(&[("", "a")]),
            keys(&[("has space", "a")]),
            keys(&[("path/like", "a")]),
            keys(&[(&"x".repeat(MAX_ACTION_CHARS + 1), "a")]),
            keys(&[("a", "")]),
            keys(&[("a", &"k".repeat(MAX_KEY_CHARS + 1))]),
            keys(&[("a", "\n")]),
            keys(&[("a", "\u{7f}")]),
            // Two actions on one key, also when only the case differs.
            keys(&[("a", "q"), ("b", "q")]),
            keys(&[("a", "q"), ("b", "Q")]),
        ];
        for map in bad {
            let err = store.update(&with_keys(map.clone())).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{map:?}");
        }
        assert_eq!(store.get().keys, kept, "a refused patch changes nothing");
        let (again, _) = SettingsStore::load(&path);
        assert_eq!(again.get().keys, kept);
    }

    #[test]
    fn a_full_map_of_sensible_names_and_every_kind_of_key_is_accepted() {
        let path = temp_file("full-keys");
        let (store, _) = SettingsStore::load(&path);
        let full: BTreeMap<String, String> = (0..MAX_KEYS)
            .map(|i| {
                (
                    format!("develop.action-{i}"),
                    format!("{}", char::from_u32(0x4e00 + i as u32).unwrap()),
                )
            })
            .collect();
        assert_eq!(store.update(&with_keys(full.clone())).unwrap().keys, full);
        let named = keys(&[("a.b", "Escape"), ("c_d", "["), ("e-f", "\\"), ("g", "é")]);
        assert_eq!(store.update(&with_keys(named.clone())).unwrap().keys, named);
    }
}
