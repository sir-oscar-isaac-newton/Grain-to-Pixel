//! App-level settings (docs/specification.md sections 5.4 and 10): `settings.json` in the OS
//! app-config folder. Per-library values live in the catalog's `meta` table instead.
//!
//! A file that cannot be read never stops the app: it is kept as `settings.json.bad` and the
//! defaults are used. Saving writes a temporary file and renames it over the old one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use gtp_capture::{CaptureProfile, CaptureProfilePatch};
use gtp_core::recipe::OutputSpace;
use gtp_core::{AppError, ErrorCode, ExposureInfo, Look, Recipe, RecipeField};
use serde::{Deserialize, Serialize};

use super::capture_levels::{LevelsScope, SavedLevels};
use super::export::{ExportFormat, ExportOptions, LONG_EDGE};
use super::focus::{validate_reference, Map};

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
            update_check: true,
        }
    }
}

/// A named set of export options (U-21). The name is plain text the user typed, or one of the two
/// built-in names, which are stored as English text and not translated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ExportPreset {
    pub name: String,
    pub options: ExportOptions,
}

/// The most Develop presets the settings keep.
pub const MAX_DEVELOP_PRESETS: usize = 200;

/// A named group of Develop settings (U-28): the recipe of the edit it was saved from, and the
/// parts of it that applying the preset copies. The name is plain text the user typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DevelopPreset {
    pub name: String,
    pub fields: Vec<RecipeField>,
    pub recipe: Recipe,
}

/// The options of the last export started, and the preset they were chosen from, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LastExport {
    pub options: ExportOptions,
    pub preset: Option<String>,
}

/// The file name template of the built-in presets.
const DEFAULT_TEMPLATE: &str = "{roll}_{index:03}";

/// The two presets a settings file without any starts with: an archive copy and a copy to share.
/// Neither names a folder, so choosing one keeps the folder already chosen.
pub fn default_export_presets() -> Vec<ExportPreset> {
    let preset = |name: &str, format, space, jpeg_quality, long_edge| ExportPreset {
        name: name.to_owned(),
        options: ExportOptions {
            format,
            space,
            jpeg_quality,
            dest_dir: String::new(),
            template: DEFAULT_TEMPLATE.to_owned(),
            write_metadata: true,
            long_edge,
        },
    };
    vec![
        preset(
            "Archive (TIFF 16-bit)",
            ExportFormat::Tiff16,
            OutputSpace::Prophoto,
            92,
            None,
        ),
        preset(
            "Share (JPEG sRGB)",
            ExportFormat::Jpeg,
            OutputSpace::Srgb,
            90,
            Some(2048),
        ),
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Settings {
    /// The library opened last, reopened by the UI at start.
    pub last_library: Option<String>,
    pub window: Option<WindowSize>,
    pub theme: Theme,
    /// What the automatic conversion of frames imported from now on starts from.
    pub starting_look: Look,
    pub flags: Flags,
    /// The keyboard shortcuts the user changed, action name to key (`Q`, `[`, `Escape`). An action
    /// that is not here has its default key, which the UI knows; the app only keeps the shape.
    pub keys: BTreeMap<String, String>,
    /// The guided capture settings. Read and changed through `capture_profile_get` and
    /// `capture_profile_set` (D-04), so they are not part of this type in the bindings.
    #[cfg_attr(feature = "specta", specta(skip))]
    pub capture: CaptureProfile,
    /// The scanlight levels saved per film stock and the camera settings they were saved with
    /// (H-15); the global default levels are `capture.levels`. Read and changed through
    /// `capture_levels_get` and `capture_levels_save`.
    #[cfg_attr(feature = "specta", specta(skip))]
    pub saved_levels: SavedLevels,
    /// When the update check last got an answer, in seconds since 1970; none before the first.
    /// Kept by the app, not by a patch, and not shown to the UI.
    #[cfg_attr(feature = "specta", specta(skip))]
    pub last_update_check: Option<u64>,
    /// The flat reference of the focus check (H-19): the 3 x 3 sharpness map of a flat target,
    /// relative to its best region, which later maps are divided by to leave what is the film's.
    /// Saved and cleared through `settings_set` (`focus_reference`, `clear_focus_reference`).
    pub focus_reference: Option<Map>,
    /// The export presets. A file without this key gets the two built-in ones; an empty list stays
    /// empty, so deleting both sticks.
    #[serde(default = "default_export_presets")]
    pub export_presets: Vec<ExportPreset>,
    /// What the export dialog starts from: the options of the last export and their preset.
    pub last_export: Option<LastExport>,
    /// The Develop presets (U-28). A file without this key has none; a preset that cannot be read
    /// (a part name from a newer version, say) is dropped on load and the rest are kept.
    #[serde(deserialize_with = "tolerant_develop_presets")]
    #[cfg_attr(feature = "specta", specta(type = Vec<DevelopPreset>))]
    pub develop_presets: Vec<DevelopPreset>,
}

/// Reads the list of presets entry by entry, so one that cannot be read does not make the whole
/// settings file unreadable. Used for this field only.
fn tolerant_develop_presets<'de, D>(deserializer: D) -> Result<Vec<DevelopPreset>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let entries = Vec::<serde_json::Value>::deserialize(deserializer)?;
    let mut presets = Vec::with_capacity(entries.len());
    for entry in entries {
        match serde_json::from_value::<DevelopPreset>(entry) {
            Ok(preset) => presets.push(preset),
            Err(err) => tracing::warn!("a Develop preset in the settings was dropped: {err}"),
        }
    }
    Ok(presets)
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            last_library: None,
            window: None,
            theme: Theme::default(),
            starting_look: Look::default(),
            flags: Flags::default(),
            keys: BTreeMap::new(),
            capture: CaptureProfile::default(),
            saved_levels: SavedLevels::default(),
            last_update_check: None,
            focus_reference: None,
            export_presets: default_export_presets(),
            last_export: None,
            develop_presets: Vec::new(),
        }
    }
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
    pub update_check: Option<bool>,
}

/// Settings to change; fields left out keep their value. The last library is set by opening one.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct SettingsPatch {
    pub window: Option<WindowSize>,
    pub theme: Option<Theme>,
    pub starting_look: Option<Look>,
    pub flags: Option<FlagsPatch>,
    /// Replaces all the key bindings (an empty map goes back to the defaults).
    pub keys: Option<BTreeMap<String, String>>,
    /// Saves this map as the focus check's flat reference (all nine values finite and above zero).
    pub focus_reference: Option<Map>,
    /// Clears the focus check's flat reference. Not together with `focus_reference`.
    pub clear_focus_reference: Option<bool>,
    /// Replaces the whole list of export presets (names are trimmed, and must be unique without
    /// regard to case).
    pub export_presets: Option<Vec<ExportPreset>>,
    /// Records the options of the export just started and the preset they came from.
    pub last_export: Option<LastExport>,
    /// Replaces the whole list of Develop presets (names are trimmed, and must be unique without
    /// regard to case; each needs at least one part, none twice; at most
    /// [`MAX_DEVELOP_PRESETS`]). The stored recipes are clamped.
    pub develop_presets: Option<Vec<DevelopPreset>>,
}

/// Refuses a JPEG quality outside 1 to 100 and a long edge outside [`LONG_EDGE`].
fn check_quality(options: &ExportOptions) -> Result<(), AppError> {
    if !(1..=100).contains(&options.jpeg_quality) {
        return Err(
            AppError::new(ErrorCode::InvalidInput, "JPEG quality runs from 1 to 100.")
                .with_details(options.jpeg_quality.to_string()),
        );
    }
    match options.long_edge {
        Some(edge) if !LONG_EDGE.contains(&edge) => Err(AppError::new(
            ErrorCode::InvalidInput,
            "The long edge runs from 256 to 20000 pixels.",
        )
        .with_details(edge.to_string())),
        _ => Ok(()),
    }
}

/// The presets with trimmed names, or why they are refused: an empty or repeated name (compared
/// without case), a quality outside 1 to 100, or a file name template that is empty or names a
/// folder. The folder may be empty.
fn checked_presets(presets: &[ExportPreset]) -> Result<Vec<ExportPreset>, AppError> {
    let invalid = |message: &str, details: String| {
        AppError::new(ErrorCode::InvalidInput, message).with_details(details)
    };
    let mut seen: Vec<String> = Vec::new();
    let mut checked = Vec::with_capacity(presets.len());
    for preset in presets {
        let name = preset.name.trim();
        if name.is_empty() {
            return Err(invalid("A preset needs a name.", String::new()));
        }
        let key = name.to_lowercase();
        if seen.contains(&key) {
            return Err(invalid(
                "Two presets cannot have the same name.",
                name.to_owned(),
            ));
        }
        seen.push(key);
        check_quality(&preset.options)?;
        let template = &preset.options.template;
        if template.trim().is_empty() || template.contains(['/', '\\']) {
            return Err(invalid(
                "A preset needs a file name without folders.",
                format!("{name}: {template:?}"),
            ));
        }
        checked.push(ExportPreset {
            name: name.to_owned(),
            options: preset.options.clone(),
        });
    }
    Ok(checked)
}

/// The parts of a recipe in the order the Develop view lists them.
const FIELD_ORDER: [RecipeField; 10] = [
    RecipeField::Geometry,
    RecipeField::Base,
    RecipeField::Invert,
    RecipeField::Color,
    RecipeField::Mixer,
    RecipeField::SplitTone,
    RecipeField::Tone,
    RecipeField::Dust,
    RecipeField::Noise,
    RecipeField::Output,
];

/// The Develop presets with trimmed names, their parts in order and their recipes clamped, or why
/// they are refused: too many, an empty or repeated name (compared without case), no part, or a
/// part listed twice.
fn checked_develop_presets(presets: &[DevelopPreset]) -> Result<Vec<DevelopPreset>, AppError> {
    let invalid = |message: &str, details: String| {
        AppError::new(ErrorCode::InvalidInput, message).with_details(details)
    };
    if presets.len() > MAX_DEVELOP_PRESETS {
        return Err(invalid(
            "There are too many presets.",
            format!("{} of at most {MAX_DEVELOP_PRESETS}", presets.len()),
        ));
    }
    let mut seen: Vec<String> = Vec::new();
    let mut checked = Vec::with_capacity(presets.len());
    for preset in presets {
        let name = preset.name.trim();
        if name.is_empty() {
            return Err(invalid("A preset needs a name.", String::new()));
        }
        let key = name.to_lowercase();
        if seen.contains(&key) {
            return Err(invalid(
                "Two presets cannot have the same name.",
                name.to_owned(),
            ));
        }
        seen.push(key);
        if preset.fields.is_empty() {
            return Err(invalid(
                "A preset needs at least one part.",
                name.to_owned(),
            ));
        }
        let fields: Vec<RecipeField> = FIELD_ORDER
            .into_iter()
            .filter(|field| preset.fields.contains(field))
            .collect();
        if fields.len() != preset.fields.len() {
            return Err(invalid(
                "A preset cannot list a part twice.",
                name.to_owned(),
            ));
        }
        checked.push(DevelopPreset {
            name: name.to_owned(),
            fields,
            recipe: preset.recipe.clone().clamped(),
        });
    }
    Ok(checked)
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
        if let Some(look) = patch.starting_look {
            self.starting_look = look;
        }
        if let Some(keys) = &patch.keys {
            check_keys(keys)?;
            self.keys = keys.clone();
        }
        match (&patch.focus_reference, patch.clear_focus_reference) {
            (Some(_), Some(true)) => {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    "The flat reference cannot be saved and cleared at once.",
                ));
            }
            (Some(map), _) => {
                validate_reference(map)?;
                self.focus_reference = Some(*map);
            }
            (None, Some(true)) => self.focus_reference = None,
            (None, _) => {}
        }
        if let Some(presets) = &patch.export_presets {
            self.export_presets = checked_presets(presets)?;
        }
        if let Some(presets) = &patch.develop_presets {
            self.develop_presets = checked_develop_presets(presets)?;
        }
        if let Some(last) = &patch.last_export {
            check_quality(&last.options)?;
            self.last_export = Some(last.clone());
        }
        if let Some(f) = patch.flags {
            let flags = &mut self.flags;
            for (slot, value) in [
                (&mut flags.dust_removal, f.dust_removal),
                (&mut flags.triplet_import, f.triplet_import),
                (&mut flags.guided_capture, f.guided_capture),
                (&mut flags.dev_overlay, f.dev_overlay),
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
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(mut settings) => {
                    // A hand-edited profile the app would refuse is replaced, not trusted.
                    let note = settings.capture.validate().err().map(|err| {
                        settings.capture = CaptureProfile::default();
                        format!("capture settings invalid ({err}); using the defaults for them")
                    });
                    // Likewise for hand-edited saved levels.
                    let levels_note = settings.saved_levels.validate().err().map(|err| {
                        settings.saved_levels = SavedLevels::default();
                        format!("saved levels invalid ({err}); using none")
                    });
                    // And a flat reference that is not finite and above zero.
                    let reference_note = settings
                        .focus_reference
                        .as_ref()
                        .and_then(|map| validate_reference(map).err())
                        .map(|err| {
                            settings.focus_reference = None;
                            format!("focus reference invalid ({err}); using none")
                        });
                    (settings, note.or(levels_note).or(reference_note))
                }
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

    /// The capture profile.
    pub fn capture_profile(&self) -> CaptureProfile {
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .capture
            .clone()
    }

    /// Applies `patch` to the capture profile, saves and returns the new profile. Nothing changes
    /// if the patch makes the profile invalid or the file cannot be written.
    pub fn update_capture(&self, patch: &CaptureProfilePatch) -> Result<CaptureProfile, AppError> {
        self.change(|settings| {
            settings.capture = settings.capture.patched(patch)?;
            Ok(())
        })
        .map(|settings| settings.capture)
    }

    /// The flat reference of the focus check, if one is saved.
    pub fn focus_reference(&self) -> Option<Map> {
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .focus_reference
    }

    /// The levels saved per film stock, and the default's camera settings.
    pub fn saved_levels(&self) -> SavedLevels {
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .saved_levels
            .clone()
    }

    /// Saves `levels` as the global default or for a film stock, with the camera settings of the
    /// test shot they were balanced on. Nothing changes if it is refused or the file cannot be
    /// written.
    pub fn save_levels(
        &self,
        scope: &LevelsScope,
        levels: [u8; 3],
        exposure: Option<ExposureInfo>,
    ) -> Result<(), AppError> {
        self.change(|settings| {
            match scope {
                LevelsScope::Default => {
                    settings.capture.levels = levels;
                    settings.saved_levels.default_exposure = exposure;
                }
                LevelsScope::Stock(name) => {
                    settings.saved_levels.save_stock(name, levels, exposure)?
                }
            }
            Ok(())
        })
        .map(|_| ())
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
        assert!(!flags.dev_overlay);
        assert!(flags.update_check);
        assert_eq!(Settings::default().theme, Theme::System);
        assert_eq!(Settings::default().last_update_check, None);
    }

    #[test]
    fn a_settings_file_with_the_removed_auto_levels_flag_still_loads() {
        let path = temp_file("removed-flag");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"theme":"dark","flags":{"dev_overlay":true,"experimental_auto_levels":true,"update_check":false}}"#,
        )
        .unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_none(), "the file is read, not set aside: {note:?}");
        let settings = store.get();
        assert_eq!(settings.theme, Theme::Dark);
        assert!(settings.flags.dev_overlay && !settings.flags.update_check);
        assert!(
            settings.flags.dust_removal,
            "flags left out keep their default"
        );
        assert!(!path.with_extension("json.bad").exists());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
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
    fn the_starting_look_is_neutral_until_chosen_and_is_kept() {
        let path = temp_file("look");
        let (store, _) = SettingsStore::load(&path);
        assert_eq!(store.get().starting_look, Look::Neutral);
        let chosen = SettingsPatch {
            starting_look: Some(Look::FilmLog),
            ..SettingsPatch::default()
        };
        assert_eq!(store.update(&chosen).unwrap().starting_look, Look::FilmLog);
        // A patch that leaves it out keeps it, and a reload brings it back.
        let theme_only = SettingsPatch {
            theme: Some(Theme::Light),
            ..SettingsPatch::default()
        };
        assert_eq!(
            store.update(&theme_only).unwrap().starting_look,
            Look::FilmLog
        );
        let (reloaded, _) = SettingsStore::load(&path);
        assert_eq!(reloaded.get().starting_look, Look::FilmLog);
        // A file written before the look existed has none, which is Neutral.
        std::fs::write(&path, r#"{"theme":"dark"}"#).unwrap();
        let (older, _) = SettingsStore::load(&path);
        assert_eq!(older.get().starting_look, Look::Neutral);
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

    #[test]
    fn the_capture_profile_defaults_and_a_patch_changes_only_its_fields_and_is_kept() {
        let path = temp_file("capture");
        let (store, _) = SettingsStore::load(&path);
        assert_eq!(store.capture_profile(), CaptureProfile::default());
        let patch = CaptureProfilePatch {
            levels: Some([60, 30, 4]),
            temp_abort_c: Some(50.0),
            ..CaptureProfilePatch::default()
        };
        let updated = store.update_capture(&patch).unwrap();
        assert_eq!(updated.levels, [60, 30, 4]);
        assert_eq!(
            CaptureProfile {
                levels: [44, 22, 2],
                temp_abort_c: 55.0,
                ..updated.clone()
            },
            CaptureProfile::default()
        );
        // Other settings are untouched, and it survives a reload.
        assert_eq!(store.get().theme, Theme::System);
        let (reloaded, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        assert_eq!(reloaded.capture_profile(), updated);
        assert!(std::fs::read_to_string(&path).unwrap().contains("capture"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_refused_capture_patch_changes_and_saves_nothing() {
        let path = temp_file("capture-refused");
        let (store, _) = SettingsStore::load(&path);
        let good = store
            .update_capture(&CaptureProfilePatch {
                settle_ms: Some(700),
                ..CaptureProfilePatch::default()
            })
            .unwrap();
        for patch in [
            CaptureProfilePatch {
                levels: Some([300, 0, 0]),
                ..CaptureProfilePatch::default()
            },
            CaptureProfilePatch {
                temp_warn_c: Some(55.0),
                ..CaptureProfilePatch::default()
            },
            CaptureProfilePatch {
                temp_abort_c: Some(f32::NAN),
                ..CaptureProfilePatch::default()
            },
        ] {
            let err = store.update_capture(&patch).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
        }
        assert_eq!(store.capture_profile(), good);
        let (again, _) = SettingsStore::load(&path);
        assert_eq!(again.capture_profile(), good);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_damaged_settings_file_gives_the_default_profile_and_a_hand_edited_invalid_one_is_replaced()
    {
        let path = temp_file("capture-damaged");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_some_and(|n| n.contains(".bad")));
        assert_eq!(store.capture_profile(), CaptureProfile::default());

        // Valid JSON, but a profile the app would refuse: the rest of the file is kept.
        std::fs::write(
            &path,
            r#"{"theme": "dark", "capture": {"temp_warn_c": 60.0, "temp_abort_c": 50.0}}"#,
        )
        .unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_some_and(|n| n.contains("capture settings invalid")));
        assert_eq!(store.capture_profile(), CaptureProfile::default());
        assert_eq!(store.get().theme, Theme::Dark);

        // A partial valid profile fills the rest with defaults.
        std::fs::write(&path, r#"{"capture": {"settle_ms": 900}}"#).unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        assert_eq!(store.capture_profile().settle_ms, 900);
        assert_eq!(store.capture_profile().levels, [44, 22, 2]);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_old_file_without_saved_levels_loads_with_none() {
        let path = temp_file("levels-old");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"capture": {"levels": [50, 25, 3]}}"#).unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        assert_eq!(store.saved_levels(), SavedLevels::default());
        assert_eq!(store.capture_profile().levels, [50, 25, 3]);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn levels_saved_as_default_or_for_a_stock_are_kept_across_a_reload() {
        let path = temp_file("levels-save");
        let (store, _) = SettingsStore::load(&path);
        let exposure = ExposureInfo {
            iso: Some(200),
            ..ExposureInfo::default()
        };
        store
            .save_levels(&LevelsScope::Default, [70, 35, 5], Some(exposure))
            .unwrap();
        store
            .save_levels(
                &LevelsScope::Stock(" Portra 400 ".into()),
                [90, 40, 6],
                None,
            )
            .unwrap();
        let (reloaded, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        assert_eq!(reloaded.capture_profile().levels, [70, 35, 5]);
        let saved = reloaded.saved_levels();
        assert_eq!(saved.default_exposure, Some(exposure));
        assert_eq!(saved.stocks["portra 400"].levels, [90, 40, 6]);
        assert_eq!(saved.stocks["portra 400"].name, "Portra 400");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn saving_levels_for_an_empty_stock_changes_and_saves_nothing() {
        let path = temp_file("levels-refused");
        let (store, _) = SettingsStore::load(&path);
        let before = store.get();
        let err = store
            .save_levels(&LevelsScope::Stock("  ".into()), [1, 2, 3], None)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert_eq!(store.get(), before);
        assert!(!path.exists());
    }

    #[test]
    fn a_focus_reference_is_saved_cleared_validated_and_kept_across_a_reload() {
        let path = temp_file("focus-reference");
        let (store, _) = SettingsStore::load(&path);
        assert_eq!(store.focus_reference(), None);
        let map = [[0.7, 0.8, 0.7], [0.8, 1.0, 0.8], [0.7, 0.8, 0.7]];
        let saved = store
            .update(&SettingsPatch {
                focus_reference: Some(map),
                ..SettingsPatch::default()
            })
            .unwrap();
        assert_eq!(saved.focus_reference, Some(map));
        let (reloaded, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        assert_eq!(reloaded.focus_reference(), Some(map));

        // A bad map, or save and clear at once, changes nothing.
        for patch in [
            SettingsPatch {
                focus_reference: Some([[0.0; 3]; 3]),
                ..SettingsPatch::default()
            },
            SettingsPatch {
                focus_reference: Some([[f32::NAN; 3]; 3]),
                ..SettingsPatch::default()
            },
            SettingsPatch {
                focus_reference: Some(map),
                clear_focus_reference: Some(true),
                ..SettingsPatch::default()
            },
        ] {
            assert_eq!(
                store.update(&patch).unwrap_err().code,
                ErrorCode::InvalidInput
            );
            assert_eq!(store.focus_reference(), Some(map));
        }

        // Another change leaves it; clearing removes it, also from the file.
        store
            .update(&SettingsPatch {
                theme: Some(Theme::Dark),
                ..SettingsPatch::default()
            })
            .unwrap();
        assert_eq!(store.focus_reference(), Some(map));
        store
            .update(&SettingsPatch {
                clear_focus_reference: Some(true),
                ..SettingsPatch::default()
            })
            .unwrap();
        assert_eq!(SettingsStore::load(&path).0.focus_reference(), None);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_old_file_has_no_focus_reference_and_a_hand_edited_bad_one_is_dropped() {
        let path = temp_file("focus-reference-load");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"theme": "dark"}"#).unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        assert_eq!(store.focus_reference(), None);
        std::fs::write(
            &path,
            r#"{"theme": "dark", "focus_reference": [[1, 1, 1], [1, -2, 1], [1, 1, 1]]}"#,
        )
        .unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_some_and(|n| n.contains("focus reference invalid")));
        assert_eq!(store.focus_reference(), None);
        assert_eq!(store.get().theme, Theme::Dark);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn hand_edited_saved_levels_the_app_would_refuse_are_dropped_on_load() {
        let path = temp_file("levels-bad");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"theme": "dark", "saved_levels": {"stocks": {"wrong": {"name": "Portra", "levels": [1, 2, 3]}}}}"#,
        )
        .unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_some_and(|n| n.contains("saved levels invalid")));
        assert_eq!(store.saved_levels(), SavedLevels::default());
        assert_eq!(store.get().theme, Theme::Dark);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// Removes every `write_metadata` key, as in a settings file written before the option.
    fn without_write_metadata(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("write_metadata");
                map.values_mut().for_each(without_write_metadata);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(without_write_metadata),
            _ => {}
        }
    }

    #[test]
    fn export_options_saved_before_write_metadata_load_with_it_on() {
        let path = temp_file("write-metadata");
        let mut settings = Settings::default();
        let mut off = preset("Off", ExportFormat::Jpeg);
        off.options.write_metadata = false;
        settings.export_presets.push(off);
        settings.last_export = Some(LastExport {
            options: ExportOptions {
                write_metadata: false,
                ..preset("x", ExportFormat::Tiff16).options
            },
            preset: None,
        });
        // With the key present, false stays false.
        let mut json = serde_json::to_value(&settings).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, json.to_string()).unwrap();
        let (kept, _) = SettingsStore::load(&path);
        assert!(!kept.get().last_export.unwrap().options.write_metadata);
        assert!(!kept.get().export_presets[2].options.write_metadata);

        // Without the key, the presets and the last export load with it on.
        without_write_metadata(&mut json);
        std::fs::write(&path, json.to_string()).unwrap();
        let (old, note) = SettingsStore::load(&path);
        assert!(note.is_none(), "{note:?}");
        let loaded = old.get();
        assert!(loaded.last_export.unwrap().options.write_metadata);
        assert_eq!(loaded.export_presets.len(), 3);
        assert!(loaded
            .export_presets
            .iter()
            .all(|p| p.options.write_metadata));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    fn preset(name: &str, format: ExportFormat) -> ExportPreset {
        ExportPreset {
            name: name.to_owned(),
            options: ExportOptions {
                format,
                space: OutputSpace::Srgb,
                jpeg_quality: 80,
                dest_dir: "D:/Out".to_owned(),
                template: "{roll}-{index}".to_owned(),
                write_metadata: true,
                long_edge: None,
            },
        }
    }

    #[test]
    fn the_share_preset_has_a_long_edge_and_the_archive_preset_none() {
        let presets = default_export_presets();
        assert_eq!(presets[0].name, "Archive (TIFF 16-bit)");
        assert_eq!(presets[0].options.long_edge, None);
        assert_eq!(presets[1].name, "Share (JPEG sRGB)");
        assert_eq!(presets[1].options.long_edge, Some(2048));
    }

    #[test]
    fn presets_saved_before_the_long_edge_load_with_none_and_are_not_changed() {
        let path = temp_file("long-edge-old");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = r#"{"export_presets":[{"name":"Mine","options":{"format":"jpeg",
            "space":"srgb","jpeg_quality":80,"dest_dir":"","template":"{roll}"}}],
            "last_export":{"options":{"format":"jpeg","space":"srgb","jpeg_quality":80,
            "dest_dir":"","template":"{roll}"},"preset":null}}"#;
        std::fs::write(&path, old).unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_none(), "{note:?}");
        let loaded = store.get();
        assert_eq!(loaded.export_presets.len(), 1);
        assert_eq!(loaded.export_presets[0].options.long_edge, None);
        assert_eq!(loaded.last_export.unwrap().options.long_edge, None);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_long_edge_outside_the_range_is_refused_in_presets_and_the_last_export() {
        let path = temp_file("long-edge-range");
        let (store, _) = SettingsStore::load(&path);
        let before = store.get();
        let edge = |edge: u32| ExportOptions {
            long_edge: Some(edge),
            ..preset("x", ExportFormat::Jpeg).options
        };
        for bad in [0, 255, 20_001, u32::MAX] {
            let named = ExportPreset {
                name: "Web".into(),
                options: edge(bad),
            };
            let err = store.update(&with_presets(vec![named])).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{bad}");
            let last = SettingsPatch {
                last_export: Some(LastExport {
                    options: edge(bad),
                    preset: None,
                }),
                ..SettingsPatch::default()
            };
            assert_eq!(
                store.update(&last).unwrap_err().code,
                ErrorCode::InvalidInput
            );
        }
        assert_eq!(store.get(), before);
        for good in [256, 2048, 20_000] {
            let named = ExportPreset {
                name: format!("Web {good}"),
                options: edge(good),
            };
            store.update(&with_presets(vec![named])).unwrap();
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    fn with_presets(presets: Vec<ExportPreset>) -> SettingsPatch {
        SettingsPatch {
            export_presets: Some(presets),
            ..SettingsPatch::default()
        }
    }

    #[test]
    fn an_old_file_gets_the_two_default_presets_but_an_empty_list_stays_empty() {
        let path = temp_file("presets-old");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"theme":"dark"}"#).unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        let presets = store.get().export_presets;
        assert_eq!(presets, default_export_presets());
        assert_eq!(Settings::default().export_presets, presets);
        let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Archive (TIFF 16-bit)", "Share (JPEG sRGB)"]);
        assert_eq!(presets[0].options.format, ExportFormat::Tiff16);
        assert_eq!(presets[0].options.space, OutputSpace::Prophoto);
        assert_eq!(presets[0].options.jpeg_quality, 92);
        assert_eq!(presets[1].options.format, ExportFormat::Jpeg);
        assert_eq!(presets[1].options.space, OutputSpace::Srgb);
        assert_eq!(presets[1].options.jpeg_quality, 90);
        assert!(presets.iter().all(|p| p.options.dest_dir.is_empty()));
        assert!(presets
            .iter()
            .all(|p| p.options.template == "{roll}_{index:03}"));
        assert!(presets.iter().all(|p| p.options.write_metadata));
        assert_eq!(store.get().last_export, None);

        std::fs::write(&path, r#"{"export_presets":[]}"#).unwrap();
        let (empty, _) = SettingsStore::load(&path);
        assert!(empty.get().export_presets.is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn presets_are_saved_updated_and_deleted_through_a_patch_and_kept_across_a_reload() {
        let path = temp_file("presets-crud");
        let (store, _) = SettingsStore::load(&path);
        let mut list = store.get().export_presets;
        list.push(preset("  Web  ", ExportFormat::Jpeg));
        let saved = store.update(&with_presets(list)).unwrap();
        assert_eq!(saved.export_presets.len(), 3);
        assert_eq!(
            saved.export_presets[2].name, "Web",
            "the name is stored trimmed"
        );

        // Update one, with an empty folder (allowed).
        let mut changed = saved.export_presets.clone();
        changed[2].options.jpeg_quality = 70;
        changed[2].options.dest_dir = String::new();
        store.update(&with_presets(changed.clone())).unwrap();

        let last = LastExport {
            options: changed[2].options.clone(),
            preset: Some("Web".to_owned()),
        };
        store
            .update(&SettingsPatch {
                last_export: Some(last.clone()),
                ..SettingsPatch::default()
            })
            .unwrap();
        let (reloaded, note) = SettingsStore::load(&path);
        assert!(note.is_none());
        assert_eq!(reloaded.get().export_presets, changed);
        assert_eq!(reloaded.get().last_export, Some(last));

        // Delete all of them: the empty list sticks after a reload, and last_export is untouched.
        store.update(&with_presets(Vec::new())).unwrap();
        let (reloaded, _) = SettingsStore::load(&path);
        assert!(reloaded.get().export_presets.is_empty());
        assert!(reloaded.get().last_export.is_some());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn refused_presets_and_last_exports_change_and_save_nothing() {
        let path = temp_file("presets-refused");
        let (store, _) = SettingsStore::load(&path);
        let before = store.get();
        let with = |edit: fn(&mut ExportPreset)| {
            let mut p = preset("Web", ExportFormat::Jpeg);
            edit(&mut p);
            p
        };
        let bad: Vec<Vec<ExportPreset>> = vec![
            vec![
                preset("Web", ExportFormat::Jpeg),
                preset(" web ", ExportFormat::Tiff16),
            ],
            vec![preset("   ", ExportFormat::Jpeg)],
            vec![with(|p| p.options.jpeg_quality = 0)],
            vec![with(|p| p.options.jpeg_quality = 101)],
            vec![with(|p| p.options.template = " ".into())],
            vec![with(|p| p.options.template = "a/b".into())],
            vec![with(|p| p.options.template = "a\\b".into())],
        ];
        for list in bad {
            let err = store.update(&with_presets(list.clone())).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{list:?}");
        }
        let bad_last = SettingsPatch {
            last_export: Some(LastExport {
                options: ExportOptions {
                    jpeg_quality: 0,
                    ..preset("x", ExportFormat::Jpeg).options
                },
                preset: None,
            }),
            ..SettingsPatch::default()
        };
        assert_eq!(
            store.update(&bad_last).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        assert_eq!(store.get(), before);
        assert!(!path.exists());
    }

    fn develop_preset(name: &str, fields: &[RecipeField]) -> DevelopPreset {
        let mut recipe = Recipe::default();
        recipe.tone.exposure_ev = 0.5;
        DevelopPreset {
            name: name.into(),
            fields: fields.to_vec(),
            recipe,
        }
    }

    fn with_develop_presets(presets: Vec<DevelopPreset>) -> SettingsPatch {
        SettingsPatch {
            develop_presets: Some(presets),
            ..SettingsPatch::default()
        }
    }

    #[test]
    fn there_are_no_develop_presets_by_default_or_in_an_old_file() {
        assert!(Settings::default().develop_presets.is_empty());
        let path = temp_file("develop-presets-old");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"theme":"dark"}"#).unwrap();
        let (store, note) = SettingsStore::load(&path);
        assert!(note.is_none(), "{note:?}");
        assert!(store.get().develop_presets.is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn develop_presets_are_saved_trimmed_ordered_and_kept_across_a_reload() {
        let path = temp_file("develop-presets");
        let (store, _) = SettingsStore::load(&path);
        let saved = store
            .update(&with_develop_presets(vec![
                develop_preset(" Warm ", &[RecipeField::Tone, RecipeField::Color]),
                develop_preset("Cool", &[RecipeField::SplitTone]),
            ]))
            .unwrap();
        assert_eq!(saved.develop_presets.len(), 2);
        assert_eq!(saved.develop_presets[0].name, "Warm");
        assert_eq!(
            saved.develop_presets[0].fields,
            vec![RecipeField::Color, RecipeField::Tone],
            "parts are stored in the enum's order"
        );
        assert_eq!(saved.develop_presets[0].recipe.tone.exposure_ev, 0.5);
        // A patch without the field leaves them alone; an empty list clears them.
        store
            .update(&SettingsPatch {
                theme: Some(Theme::Dark),
                ..SettingsPatch::default()
            })
            .unwrap();
        let (reloaded, _) = SettingsStore::load(&path);
        assert_eq!(reloaded.get().develop_presets, saved.develop_presets);
        reloaded.update(&with_develop_presets(Vec::new())).unwrap();
        assert!(reloaded.get().develop_presets.is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn refused_develop_presets_change_and_save_nothing() {
        let path = temp_file("develop-presets-refused");
        let (store, _) = SettingsStore::load(&path);
        let before = store.get();
        let tone = [RecipeField::Tone];
        let bad: Vec<Vec<DevelopPreset>> = vec![
            vec![develop_preset("   ", &tone)],
            vec![
                develop_preset("Warm", &tone),
                develop_preset(" WARM ", &tone),
            ],
            vec![develop_preset("Warm", &[])],
            vec![develop_preset(
                "Warm",
                &[RecipeField::Tone, RecipeField::Color, RecipeField::Tone],
            )],
            (0..=MAX_DEVELOP_PRESETS)
                .map(|i| develop_preset(&format!("P{i}"), &tone))
                .collect(),
        ];
        for list in bad {
            let err = store
                .update(&with_develop_presets(list.clone()))
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{list:?}");
        }
        assert_eq!(store.get(), before);
        assert!(!path.exists());

        let full: Vec<DevelopPreset> = (0..MAX_DEVELOP_PRESETS)
            .map(|i| develop_preset(&format!("P{i}"), &tone))
            .collect();
        assert_eq!(
            store
                .update(&with_develop_presets(full))
                .unwrap()
                .develop_presets
                .len(),
            MAX_DEVELOP_PRESETS
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_develop_presets_recipe_is_clamped_on_save() {
        let path = temp_file("develop-presets-clamp");
        let (store, _) = SettingsStore::load(&path);
        let mut wild = develop_preset("Wild", &[RecipeField::Tone]);
        wild.recipe.tone.exposure_ev = 5000.0;
        let saved = store
            .update(&with_develop_presets(vec![wild.clone()]))
            .unwrap();
        assert_eq!(saved.develop_presets[0].recipe, wild.recipe.clamped());
        assert_ne!(saved.develop_presets[0].recipe.tone.exposure_ev, 5000.0);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn an_unreadable_develop_preset_is_dropped_and_the_rest_of_the_file_loads() {
        let path = temp_file("develop-presets-tolerant");
        let good = develop_preset("Good", &[RecipeField::Tone]);
        let other = develop_preset("Other", &[RecipeField::Color, RecipeField::Dust]);
        let mut file = serde_json::to_value(Settings::default()).unwrap();
        let entry = |p: &DevelopPreset| serde_json::to_value(p).unwrap();
        let mut newer = entry(&good);
        newer["name"] = "Newer".into();
        newer["fields"] = serde_json::json!(["Tone", "FromTheFuture"]);
        file["develop_presets"] = serde_json::json!([entry(&good), newer, "junk", entry(&other)]);
        file["theme"] = "dark".into();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, file.to_string()).unwrap();

        let (loaded, note) = SettingsStore::load(&path);
        assert!(note.is_none(), "the file is read, not set aside: {note:?}");
        let settings = loaded.get();
        assert_eq!(settings.theme, Theme::Dark);
        assert_eq!(settings.develop_presets, vec![good, other]);
        assert!(!path.with_extension("json.bad").exists());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn last_export_keeps_its_folder_and_template_as_given() {
        let path = temp_file("last-export");
        let (store, _) = SettingsStore::load(&path);
        let last = LastExport {
            options: ExportOptions {
                template: "a/b".to_owned(),
                dest_dir: String::new(),
                ..preset("x", ExportFormat::Jpeg).options
            },
            preset: None,
        };
        let saved = store
            .update(&SettingsPatch {
                last_export: Some(last.clone()),
                ..SettingsPatch::default()
            })
            .unwrap();
        assert_eq!(saved.last_export, Some(last));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
