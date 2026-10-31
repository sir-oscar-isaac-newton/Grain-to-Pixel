use serde::{Deserialize, Serialize};

use crate::error::{AppError, ErrorCode};
use crate::ids::RollId;

/// Film format of a roll. Only 35 mm is supported in the MVP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum FilmFormat {
    #[default]
    #[serde(rename = "35mm")]
    Mm35,
}

/// Highest ISO accepted for a roll (box or shot); beyond any film made.
pub const MAX_ISO: u32 = 102_400;

/// A roll of film: the unit that shares one measured film base. Its tags (in the catalog) count for
/// every frame on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Roll {
    pub id: RollId,
    pub name: String,
    pub film_stock: Option<String>,
    #[serde(default)]
    pub format: FilmFormat,
    /// The camera the roll was shot in.
    #[serde(default)]
    pub camera: Option<String>,
    /// The film's rated speed (box ISO).
    #[serde(default)]
    pub iso_box: Option<u32>,
    /// The speed it was exposed at; differs from `iso_box` when pushed or pulled.
    #[serde(default)]
    pub iso_shot: Option<u32>,
    /// When shooting started and ended: partial ISO dates (`2026`, `2026-07` or `2026-07-14`),
    /// checked by [`check_shot_range`].
    #[serde(default)]
    pub shot_from: Option<String>,
    #[serde(default)]
    pub shot_to: Option<String>,
    #[serde(default)]
    pub notes: String,
    /// Measured film base in linear scan values, shared by all frames of the roll.
    pub film_base_rgb: Option<[f32; 3]>,
    /// RFC 3339 timestamp.
    pub created_at: String,
}

fn invalid_date(text: &str) -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "Dates are written as 2026, 2026-07 or 2026-07-14.",
    )
    .with_details(text.to_owned())
}

/// A calendar day as `(year, month, day)`; tuples compare in date order.
type Ymd = (u32, u32, u32);

/// The first and last day covered by a partial ISO date.
fn date_bounds(text: &str) -> Result<(Ymd, Ymd), AppError> {
    let parts: Vec<&str> = text.split('-').collect();
    let number = |part: &str, len: usize| -> Result<u32, AppError> {
        if part.len() != len || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid_date(text));
        }
        part.parse().map_err(|_| invalid_date(text))
    };
    let days_in = |year: u32, month: u32| match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    let year = number(parts.first().copied().unwrap_or(""), 4)?;
    if !(1800..=9999).contains(&year) {
        return Err(invalid_date(text));
    }
    match parts.len() {
        1 => Ok(((year, 1, 1), (year, 12, 31))),
        2 | 3 => {
            let month = number(parts[1], 2)?;
            if !(1..=12).contains(&month) {
                return Err(invalid_date(text));
            }
            if parts.len() == 2 {
                return Ok(((year, month, 1), (year, month, days_in(year, month))));
            }
            let day = number(parts[2], 2)?;
            if !(1..=days_in(year, month)).contains(&day) {
                return Err(invalid_date(text));
            }
            Ok(((year, month, day), (year, month, day)))
        }
        _ => Err(invalid_date(text)),
    }
}

/// Checks the shot dates of a roll: each is a valid partial date, and the range does not end before
/// it starts (`2026-07-14` to `2026-07` is fine: July ends after the 14th).
pub fn check_shot_range(from: Option<&str>, to: Option<&str>) -> Result<(), AppError> {
    let from = from.map(date_bounds).transpose()?;
    let to = to.map(date_bounds).transpose()?;
    if let (Some((start, _)), Some((_, end))) = (from, to) {
        if end < start {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "The roll's last shooting date is before its first.",
            ));
        }
    }
    Ok(())
}

/// Checks an ISO value (box or shot).
pub fn check_iso(iso: Option<u32>) -> Result<(), AppError> {
    match iso {
        Some(value) if !(1..=MAX_ISO).contains(&value) => Err(AppError::new(
            ErrorCode::InvalidInput,
            "That ISO is out of range.",
        )
        .with_details(value.to_string())),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_roll_round_trips_and_defaults_optional_fields() {
        let id = RollId::new();
        let json =
            format!(r#"{{"id":"{id}","name":"Roll 1","created_at":"2026-10-30T10:00:00Z"}}"#);
        let roll: Roll = serde_json::from_str(&json).unwrap();
        assert_eq!(roll.format, FilmFormat::Mm35);
        assert_eq!(roll.notes, "");
        assert!(roll.film_base_rgb.is_none());
        assert!(roll.camera.is_none() && roll.iso_box.is_none() && roll.shot_from.is_none());

        let again: Roll = serde_json::from_str(&serde_json::to_string(&roll).unwrap()).unwrap();
        assert_eq!(again, roll);
    }

    #[test]
    fn the_format_is_written_as_35mm() {
        assert_eq!(
            serde_json::to_string(&FilmFormat::Mm35).unwrap(),
            "\"35mm\""
        );
    }

    #[test]
    fn partial_dates_and_ranges_are_checked() {
        for ok in [
            (Some("2026"), None),
            (None, Some("2026-07")),
            (Some("2026-07-14"), Some("2026-07")),
            (Some("2026-07"), Some("2026-07-01")),
            (Some("2024-02-29"), Some("2025")),
        ] {
            assert!(check_shot_range(ok.0, ok.1).is_ok(), "{ok:?}");
        }
        for bad in [
            (Some("2026-7"), None),
            (Some("26"), None),
            (Some("2026-13"), None),
            (Some("2025-02-29"), None),
            (Some("2026-07-14-1"), None),
            (Some("2026/07"), None),
            (Some("2026-08"), Some("2026-07-31")),
            (Some("2027"), Some("2026")),
        ] {
            let err = check_shot_range(bad.0, bad.1).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{bad:?}");
        }
    }

    #[test]
    fn iso_must_be_positive_and_bounded() {
        assert!(check_iso(None).is_ok());
        assert!(check_iso(Some(400)).is_ok());
        assert!(check_iso(Some(0)).is_err());
        assert!(check_iso(Some(MAX_ISO + 1)).is_err());
    }
}
