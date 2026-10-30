//! Reads a stored Recipe of any known version into the current one.

use serde_json::Value;

use super::v1::Recipe;
use crate::error::{AppError, ErrorCode};

/// The Recipe version this build writes. A new version needs a migration function here.
pub const CURRENT_VERSION: u32 = 1;

fn unreadable(details: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::InvalidInput, "This edit could not be read.")
        .with_details(details.to_string())
}

/// Parses a stored recipe: version 1 is accepted (a missing `v` means 1), and the result is
/// clamped. Any other version is `UnsupportedFormat`; a newer one was made by a newer app.
pub fn migrate(json: &str) -> Result<Recipe, AppError> {
    let value: Value = serde_json::from_str(json).map_err(unreadable)?;
    let Some(object) = value.as_object() else {
        return Err(unreadable("a recipe must be a JSON object"));
    };
    let version = match object.get("v") {
        None => u64::from(CURRENT_VERSION),
        Some(v) => v
            .as_u64()
            .ok_or_else(|| unreadable("the recipe version must be a whole number"))?,
    };
    match version {
        1 => {
            let mut recipe: Recipe = serde_json::from_value(value).map_err(unreadable)?;
            recipe.v = CURRENT_VERSION;
            recipe.clamp();
            Ok(recipe)
        }
        newer if newer > u64::from(CURRENT_VERSION) => Err(AppError::new(
            ErrorCode::UnsupportedFormat,
            "This edit was made by a newer version of Grain to Pixel. Update the app to open it.",
        )
        .with_details(format!("recipe version {newer}"))),
        other => Err(AppError::new(
            ErrorCode::UnsupportedFormat,
            "This edit uses a version the app does not know.",
        )
        .with_details(format!("recipe version {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::OutputSpace;

    #[test]
    fn a_recipe_round_trips_through_json() {
        let mut recipe = Recipe::default();
        recipe.geometry.angle_deg = -2.5;
        recipe.geometry.rot90 = 1;
        recipe.color.red_cyan = 12.0;
        recipe.color.blue_yellow = -30.0;
        recipe.tone.whites = 25.0;
        recipe.tone.blacks = -10.0;
        recipe.tone.curve.master = vec![[0.0, 0.0], [0.5, 0.25], [1.0, 1.0]];
        recipe.dust.grow_px = 4;
        recipe.output.space = OutputSpace::AdobeRgb;

        let json = serde_json::to_string(&recipe).unwrap();
        assert_eq!(migrate(&json).unwrap(), recipe);
    }

    #[test]
    fn the_json_shape_matches_the_specification() {
        let value = serde_json::to_value(Recipe::default()).unwrap();
        assert_eq!(value["v"], 1);
        assert_eq!(value["geometry"]["crop"]["w"], 1.0);
        assert_eq!(value["base"]["source"], "roll");
        assert_eq!(
            value["tone"]["curve"]["master"],
            serde_json::json!([[0.0, 0.0], [1.0, 1.0]])
        );
        assert_eq!(value["color"]["saturation"], 1.0);
        assert_eq!(value["dust"]["enabled"], true);
        assert_eq!(value["output"]["space"], "srgb");
    }

    #[test]
    fn unknown_fields_are_ignored_and_missing_fields_default() {
        let recipe = migrate(
            r#"{"v":1,"surprise":true,"color":{"tint":5,"future":1},"tone":{"contrast":10}}"#,
        )
        .unwrap();
        assert_eq!(recipe.color.tint, 5.0);
        assert_eq!(recipe.tone.contrast, 10.0);
        let defaults = Recipe::default();
        assert_eq!(recipe.geometry, defaults.geometry);
        assert_eq!(recipe.color.saturation, 1.0);
        assert_eq!(recipe.dust, defaults.dust);
    }

    #[test]
    fn an_empty_object_is_the_default_recipe() {
        assert_eq!(migrate("{}").unwrap(), Recipe::default());
    }

    #[test]
    fn values_are_clamped_on_load() {
        let recipe =
            migrate(r#"{"v":1,"color":{"red_cyan":5000},"geometry":{"rot90":5}}"#).unwrap();
        assert_eq!(recipe.color.red_cyan, 100.0);
        assert_eq!(recipe.geometry.rot90, 1);
    }

    #[test]
    fn future_and_unknown_versions_are_unsupported() {
        for json in [r#"{"v":2}"#, r#"{"v":999}"#, r#"{"v":0}"#] {
            let err = migrate(json).unwrap_err();
            assert_eq!(err.code, ErrorCode::UnsupportedFormat, "{json}");
        }
        assert!(migrate(r#"{"v":2}"#).unwrap_err().message.contains("newer"));
    }

    #[test]
    fn malformed_input_is_invalid_input() {
        for json in [
            "",
            "not json",
            "[]",
            "42",
            r#"{"v":"1"}"#,
            r#"{"v":-1}"#,
            r#"{"v":1.5}"#,
            r#"{"v":1,"dust":{"grow_px":-3}}"#,
            r#"{"v":1,"output":{"space":"cmyk"}}"#,
        ] {
            let err = migrate(json).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{json}");
        }
    }
}
