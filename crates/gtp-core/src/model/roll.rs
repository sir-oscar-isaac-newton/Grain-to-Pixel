use serde::{Deserialize, Serialize};

use crate::ids::RollId;

/// Film format of a roll. Only 35 mm is supported in the MVP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FilmFormat {
    #[default]
    #[serde(rename = "35mm")]
    Mm35,
}

/// A roll of film: the unit that shares one measured film base.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Roll {
    pub id: RollId,
    pub name: String,
    pub film_stock: Option<String>,
    #[serde(default)]
    pub format: FilmFormat,
    /// ISO date (`2026-10-30`), not parsed here.
    pub shot_date: Option<String>,
    #[serde(default)]
    pub notes: String,
    /// Measured film base in linear scan values, shared by all frames of the roll.
    pub film_base_rgb: Option<[f32; 3]>,
    /// RFC 3339 timestamp.
    pub created_at: String,
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
}
