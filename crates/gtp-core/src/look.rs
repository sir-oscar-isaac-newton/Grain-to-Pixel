//! Looks: named starting points for the automatic recipe (P-19). A look is not a new stage or a new
//! recipe field: `analyze` writes the values of the look into the recipe it returns, so the recipe
//! stays complete and every value stays editable. The values themselves are computed in `gtp-edit`.

use serde::{Deserialize, Serialize};

/// What the automatic conversion starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum Look {
    /// The conversion as it has always been: density mapped linearly between the black and white
    /// points, no tone curve.
    #[default]
    Neutral,
    /// A flat, wide-latitude start encoded with the Blackmagic Film Gen 5 log curve, for editing.
    FilmLog,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_is_written_in_snake_case_and_defaults_to_neutral() {
        assert_eq!(Look::default(), Look::Neutral);
        assert_eq!(
            serde_json::to_string(&Look::FilmLog).unwrap(),
            "\"film_log\""
        );
        assert_eq!(
            serde_json::from_str::<Look>("\"neutral\"").unwrap(),
            Look::Neutral
        );
        assert!(serde_json::from_str::<Look>("\"frontier\"").is_err());
    }
}
