//! Scanlight levels, global and per film stock (H-15).
//!
//! The levels are the red, green and blue LED levels of one capture. The global default is
//! `CaptureProfile::levels`; a film stock may have levels of its own, saved with the camera
//! settings of the test shot they were balanced on. Both live in `settings.json`, so they belong to
//! the lamp and the camera of the person using the app, not to a library.
//!
//! A roll's levels are resolved from its `film_stock` (free text, compared trimmed and without
//! case), then the global default. A roll with no stock, or a stock with no saved levels, uses the
//! default. Saving is explicit: the sliders hold working levels that are only kept by
//! [`save_levels`].

use std::collections::BTreeMap;

use gtp_core::{AppError, ErrorCode, ExposureInfo, ExposureRatio, FrameId, RollId};
use gtp_ingest::{ExifInfo, Ratio};
use serde::{Deserialize, Serialize};

use crate::context::AppContext;

/// The most film stocks that can have saved levels.
pub const MAX_STOCKS: usize = 64;
/// The longest stock name kept, in characters.
pub const MAX_STOCK_CHARS: usize = 64;

/// Levels saved for one film stock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StockLevels {
    /// The stock as it was spelled when saving.
    pub name: String,
    pub levels: [u8; 3],
    /// The camera settings of the test shot the levels were saved after.
    #[serde(default)]
    pub exposure: Option<ExposureInfo>,
}

/// The levels saved besides the global default (which is `CaptureProfile::levels`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedLevels {
    /// The camera settings of the test shot the global default was saved after.
    pub default_exposure: Option<ExposureInfo>,
    /// Per stock, by [`stock_key`].
    pub stocks: BTreeMap<String, StockLevels>,
}

/// Where a roll's levels come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum LevelsSource {
    /// Saved for the roll's film stock.
    Stock,
    /// The global default.
    Default,
}

/// The levels a roll captures with, and what they were saved with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CaptureLevels {
    pub levels: [u8; 3],
    pub source: LevelsSource,
    /// The stock the levels are saved for, as it was spelled when saving (for the `stock` source).
    pub stock: Option<String>,
    /// The camera settings of the test shot these levels were saved after, when known.
    pub exposure: Option<ExposureInfo>,
}

/// Where levels are saved to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum LevelsScope {
    /// The global default.
    Default,
    /// This film stock.
    Stock(String),
}

/// A film stock as a map key: trimmed and lowercased (Unicode-aware); `None` when empty.
pub fn stock_key(name: &str) -> Option<String> {
    let trimmed = name.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_lowercase())
}

/// The levels for a roll of `film_stock`: the stock's saved levels, else `default_levels`.
pub fn resolve(
    default_levels: [u8; 3],
    saved: &SavedLevels,
    film_stock: Option<&str>,
) -> CaptureLevels {
    let hit = film_stock
        .and_then(stock_key)
        .and_then(|key| saved.stocks.get(&key));
    match hit {
        Some(stock) => CaptureLevels {
            levels: stock.levels,
            source: LevelsSource::Stock,
            stock: Some(stock.name.clone()),
            exposure: stock.exposure,
        },
        None => CaptureLevels {
            levels: default_levels,
            source: LevelsSource::Default,
            stock: None,
            exposure: saved.default_exposure,
        },
    }
}

fn invalid(message: &str, details: String) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message).with_details(details)
}

/// A stock name as it is kept: trimmed, non-empty, short, without control characters.
pub fn check_stock_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(invalid("The film stock needs a name.", String::new()));
    }
    if name.chars().count() > MAX_STOCK_CHARS || name.chars().any(char::is_control) {
        return Err(invalid(
            "That film stock name is not valid.",
            format!(
                "{} characters of at most {MAX_STOCK_CHARS}",
                name.chars().count()
            ),
        ));
    }
    Ok(name.to_owned())
}

impl SavedLevels {
    /// Checks a loaded value: few stocks, each under the key its name gives.
    pub fn validate(&self) -> Result<(), AppError> {
        if self.stocks.len() > MAX_STOCKS {
            return Err(invalid(
                "There are too many film stocks with saved levels.",
                format!("{} of at most {MAX_STOCKS}", self.stocks.len()),
            ));
        }
        for (key, stock) in &self.stocks {
            let name = check_stock_name(&stock.name)?;
            if stock_key(&name).as_deref() != Some(key.as_str()) {
                return Err(invalid(
                    "A film stock is saved under the wrong key.",
                    format!("{key:?} for {name:?}"),
                ));
            }
        }
        Ok(())
    }

    /// Saves `levels` for the stock `name` (replacing the stock's earlier levels, whatever its
    /// spelling). Refused for an invalid name, or when the stock is new and there are too many.
    pub fn save_stock(
        &mut self,
        name: &str,
        levels: [u8; 3],
        exposure: Option<ExposureInfo>,
    ) -> Result<(), AppError> {
        let name = check_stock_name(name)?;
        let key = stock_key(&name).unwrap_or_default();
        if !self.stocks.contains_key(&key) && self.stocks.len() >= MAX_STOCKS {
            return Err(invalid(
                "There are too many film stocks with saved levels.",
                format!("at most {MAX_STOCKS}"),
            ));
        }
        self.stocks.insert(
            key,
            StockLevels {
                name,
                levels,
                exposure,
            },
        );
        Ok(())
    }
}

/// Levels from the wider numbers the UI sends, so that a value outside 0 to 255 is refused with a
/// message and not as a malformed request.
pub fn levels_from_wide(levels: [u32; 3]) -> Result<[u8; 3], AppError> {
    let mut bytes = [0u8; 3];
    for (slot, level) in bytes.iter_mut().zip(levels) {
        *slot = u8::try_from(level).map_err(|_| {
            invalid(
                "A light level is 0 to 255.",
                format!("levels {}, {}, {}", levels[0], levels[1], levels[2]),
            )
        })?;
    }
    Ok(bytes)
}

/// The EXIF triple as the app records and compares it.
pub fn exposure_from_exif(info: ExifInfo) -> ExposureInfo {
    let ratio = |r: Ratio| ExposureRatio {
        num: r.num,
        den: r.den,
    };
    ExposureInfo {
        exposure_time: info.exposure_time.map(ratio),
        iso: info.iso,
        f_number: info.f_number.map(ratio),
    }
}

/// `capture_levels_get`: the levels the roll captures with.
pub fn levels_for_roll(ctx: &AppContext, roll: RollId) -> Result<CaptureLevels, AppError> {
    let roll = ctx.library()?.read(|c| c.get_roll(roll))?;
    let settings = ctx.settings();
    Ok(resolve(
        settings.capture_profile().levels,
        &settings.saved_levels(),
        roll.film_stock.as_deref(),
    ))
}

/// `capture_levels_save`: keeps `levels` as the global default or for a film stock, with the camera
/// settings of the test shot they were balanced on. Returns the levels as they now resolve for that
/// scope.
pub fn save_levels(
    ctx: &AppContext,
    scope: &LevelsScope,
    levels: [u32; 3],
    exposure: Option<ExposureInfo>,
) -> Result<CaptureLevels, AppError> {
    let levels = levels_from_wide(levels)?;
    let exposure = exposure.filter(|e| !e.is_empty());
    let store = ctx.settings();
    store.save_levels(scope, levels, exposure)?;
    let film_stock = match scope {
        LevelsScope::Default => None,
        LevelsScope::Stock(name) => Some(name.as_str()),
    };
    Ok(resolve(
        store.capture_profile().levels,
        &store.saved_levels(),
        film_stock,
    ))
}

/// Which levels a capture used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsedSource {
    Stock,
    Default,
    /// Levels that are not the saved ones for the roll (the sliders were moved).
    Manual,
}

/// The levels of one capture, where they came from and what to compare the shots with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelsUsed {
    pub levels: [u8; 3],
    pub source: UsedSource,
    pub stock: Option<String>,
    /// The camera settings the saved levels were balanced on; `None` for manual levels.
    pub saved_exposure: Option<ExposureInfo>,
}

impl LevelsUsed {
    /// Working `levels` for a roll whose saved levels are `resolved`: the saved ones when equal,
    /// manual otherwise.
    pub fn for_working(levels: [u8; 3], resolved: &CaptureLevels) -> Self {
        if levels != resolved.levels {
            return Self::manual(levels);
        }
        Self {
            levels,
            source: match resolved.source {
                LevelsSource::Stock => UsedSource::Stock,
                LevelsSource::Default => UsedSource::Default,
            },
            stock: resolved.stock.clone(),
            saved_exposure: resolved.exposure,
        }
    }

    /// Levels set by hand: nothing saved to compare with.
    pub fn manual(levels: [u8; 3]) -> Self {
        Self {
            levels,
            source: UsedSource::Manual,
            stock: None,
            saved_exposure: None,
        }
    }
}

/// What is recorded with a captured frame (`frame.capture_info`, JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureInfo {
    /// The version of this record.
    pub v: u32,
    pub levels: [u8; 3],
    pub source: UsedSource,
    pub stock: Option<String>,
    /// The camera settings of the red-, green- and blue-lit pictures.
    pub exposure: [ExposureInfo; 3],
}

impl CaptureInfo {
    pub const VERSION: u32 = 1;

    pub fn new(used: &LevelsUsed, exposure: [ExposureInfo; 3]) -> Self {
        Self {
            v: Self::VERSION,
            levels: used.levels,
            source: used.source,
            stock: used.stock.clone(),
            exposure,
        }
    }

    pub fn to_json(&self) -> Result<String, AppError> {
        serde_json::to_string(self).map_err(AppError::internal)
    }

    /// True when the levels were saved with camera settings and one of the pictures differs.
    pub fn exposure_differs(&self, used: &LevelsUsed) -> bool {
        used.source != UsedSource::Manual
            && used
                .saved_exposure
                .is_some_and(|saved| self.exposure.iter().any(|shot| shot.differs_from(&saved)))
    }
}

/// How a captured frame was lit, read back from the catalog; `None` for a frame that was not
/// captured (or was captured before H-15).
pub fn frame_capture_info(
    ctx: &AppContext,
    frame: FrameId,
) -> Result<Option<CaptureInfo>, AppError> {
    let json = ctx.library()?.read(|c| c.capture_info(frame))?;
    json.map(|text| serde_json::from_str(&text).map_err(AppError::internal))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exposure(iso: u32) -> ExposureInfo {
        ExposureInfo {
            exposure_time: Some(ExposureRatio { num: 1, den: 60 }),
            iso: Some(iso),
            f_number: Some(ExposureRatio { num: 56, den: 10 }),
        }
    }

    fn saved() -> SavedLevels {
        let mut saved = SavedLevels {
            default_exposure: Some(exposure(100)),
            ..SavedLevels::default()
        };
        saved
            .save_stock("Kodak Portra 400", [60, 30, 4], Some(exposure(200)))
            .unwrap();
        saved
    }

    #[test]
    fn a_stock_with_saved_levels_uses_them() {
        let got = resolve([44, 22, 2], &saved(), Some("Kodak Portra 400"));
        assert_eq!(got.levels, [60, 30, 4]);
        assert_eq!(got.source, LevelsSource::Stock);
        assert_eq!(got.stock.as_deref(), Some("Kodak Portra 400"));
        assert_eq!(got.exposure, Some(exposure(200)));
    }

    #[test]
    fn the_stock_is_matched_trimmed_and_without_case() {
        for spelling in ["  kodak portra 400 ", "KODAK PORTRA 400"] {
            let got = resolve([44, 22, 2], &saved(), Some(spelling));
            assert_eq!(got.source, LevelsSource::Stock, "{spelling}");
            assert_eq!(got.stock.as_deref(), Some("Kodak Portra 400"));
        }
        assert_eq!(
            stock_key("  \u{c9}KTAR 100 "),
            Some("\u{e9}ktar 100".to_owned())
        );
        assert_eq!(stock_key("   "), None);
    }

    #[test]
    fn an_unknown_stock_or_none_uses_the_default() {
        for stock in [Some("Ilford HP5"), Some("  "), None] {
            let got = resolve([44, 22, 2], &saved(), stock);
            assert_eq!(got.levels, [44, 22, 2]);
            assert_eq!(got.source, LevelsSource::Default);
            assert_eq!(got.stock, None);
            assert_eq!(got.exposure, Some(exposure(100)));
        }
    }

    #[test]
    fn saving_a_stock_again_replaces_it_whatever_the_spelling() {
        let mut saved = saved();
        saved
            .save_stock(" KODAK portra 400", [1, 2, 3], None)
            .unwrap();
        assert_eq!(saved.stocks.len(), 1);
        let got = resolve([44, 22, 2], &saved, Some("kodak portra 400"));
        assert_eq!(got.levels, [1, 2, 3]);
        assert_eq!(got.stock.as_deref(), Some("KODAK portra 400"));
        assert_eq!(got.exposure, None);
    }

    #[test]
    fn an_empty_or_bad_name_and_too_many_stocks_are_refused() {
        let mut saved = SavedLevels::default();
        for name in ["", "   ", "a\nb", &"x".repeat(MAX_STOCK_CHARS + 1)] {
            let err = saved.save_stock(name, [1, 1, 1], None).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{name:?}");
        }
        for i in 0..MAX_STOCKS {
            saved
                .save_stock(&format!("stock {i}"), [1, 1, 1], None)
                .unwrap();
        }
        assert!(saved.save_stock("one more", [1, 1, 1], None).is_err());
        saved.save_stock("STOCK 3", [9, 9, 9], None).unwrap();
        assert_eq!(saved.stocks.len(), MAX_STOCKS);
        saved.validate().unwrap();
    }

    #[test]
    fn a_hand_edited_map_under_the_wrong_key_is_invalid() {
        let mut saved = saved();
        let stock = saved.stocks.remove("kodak portra 400").unwrap();
        saved.stocks.insert("something else".into(), stock);
        assert!(saved.validate().is_err());
    }

    #[test]
    fn wide_levels_are_checked_against_a_byte() {
        assert_eq!(levels_from_wide([0, 128, 255]).unwrap(), [0, 128, 255]);
        let err = levels_from_wide([0, 256, 1]).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn working_levels_equal_to_the_saved_ones_keep_their_source_and_others_are_manual() {
        let resolved = resolve([44, 22, 2], &saved(), Some("Kodak Portra 400"));
        let same = LevelsUsed::for_working([60, 30, 4], &resolved);
        assert_eq!(same.source, UsedSource::Stock);
        assert_eq!(same.stock.as_deref(), Some("Kodak Portra 400"));
        assert_eq!(same.saved_exposure, Some(exposure(200)));
        let moved = LevelsUsed::for_working([61, 30, 4], &resolved);
        assert_eq!(moved, LevelsUsed::manual([61, 30, 4]));
        let default = resolve([44, 22, 2], &saved(), None);
        assert_eq!(
            LevelsUsed::for_working([44, 22, 2], &default).source,
            UsedSource::Default
        );
    }

    #[test]
    fn the_exposure_warning_needs_saved_settings_that_differ_and_never_applies_to_manual_levels() {
        let resolved = resolve([44, 22, 2], &saved(), Some("Kodak Portra 400"));
        let used = LevelsUsed::for_working([60, 30, 4], &resolved);
        let same = CaptureInfo::new(&used, [exposure(200); 3]);
        assert!(!same.exposure_differs(&used));
        let other = CaptureInfo::new(&used, [exposure(200), exposure(400), exposure(200)]);
        assert!(other.exposure_differs(&used));
        let unknown = CaptureInfo::new(&used, [ExposureInfo::default(); 3]);
        assert!(!unknown.exposure_differs(&used), "no EXIF says nothing");
        let manual = LevelsUsed::manual([61, 30, 4]);
        assert!(!CaptureInfo::new(&manual, [exposure(400); 3]).exposure_differs(&manual));
        let no_saved = resolve([44, 22, 2], &SavedLevels::default(), None);
        let used = LevelsUsed::for_working([44, 22, 2], &no_saved);
        assert!(!CaptureInfo::new(&used, [exposure(400); 3]).exposure_differs(&used));
    }

    #[test]
    fn the_record_round_trips_as_versioned_json() {
        let used = LevelsUsed::manual([1, 2, 3]);
        let info = CaptureInfo::new(&used, [exposure(100), exposure(100), exposure(100)]);
        let json = info.to_json().unwrap();
        assert!(json.contains("\"v\":1"), "{json}");
        assert!(json.contains("\"source\":\"manual\""), "{json}");
        assert_eq!(serde_json::from_str::<CaptureInfo>(&json).unwrap(), info);
    }
}
