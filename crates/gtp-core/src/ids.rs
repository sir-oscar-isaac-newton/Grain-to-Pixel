//! Identifiers: ULIDs (sortable, collision-safe) in one newtype per entity so they cannot be mixed up.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ulid::Ulid;

use crate::error::{AppError, ErrorCode};

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident, $label:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[cfg_attr(feature = "specta", derive(specta::Type), specta(transparent))]
        // On the IPC boundary an id is its 26-character text.
        pub struct $name(#[cfg_attr(feature = "specta", specta(type = String))] Ulid);

        impl $name {
            /// A new id, later than every id created before it.
            pub fn new() -> Self {
                Self(Ulid::new())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = AppError;

            fn from_str(text: &str) -> Result<Self, AppError> {
                Ulid::from_string(text).map(Self).map_err(|err| {
                    AppError::new(
                        ErrorCode::InvalidInput,
                        concat!("That is not a valid ", $label, " id."),
                    )
                    .with_details(err.to_string())
                })
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

id_type!(
    /// Identifies a roll of film.
    RollId,
    "roll"
);
id_type!(
    /// Identifies one frame of a roll.
    FrameId,
    "frame"
);
id_type!(
    /// Identifies a tag.
    TagId,
    "tag"
);
id_type!(
    /// Identifies a background job (import, scan, export, ...).
    JobId,
    "job"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_round_trips_through_text_and_json() {
        let id = FrameId::new();
        let text = id.to_string();
        assert_eq!(text.len(), 26);
        assert_eq!(text.parse::<FrameId>().unwrap(), id);

        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<FrameId>(&json).unwrap(), id);
    }

    #[test]
    fn malformed_ids_are_rejected_with_invalid_input() {
        for bad in [
            "",
            "not-a-ulid",
            "01ARZ3NDEKTSV4RRFFQ69G5FA",
            "01ARZ3NDEKTSV4RRFFQ69G5FAVX",
            "!!!!!!!!!!!!!!!!!!!!!!!!!!",
        ] {
            let err = bad.parse::<RollId>().unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{bad:?}");
            assert!(err.message.contains("roll"));
        }
        assert!(serde_json::from_str::<TagId>("\"nope\"").is_err());
        assert!(serde_json::from_str::<TagId>("42").is_err());
    }

    #[test]
    fn ids_sort_in_creation_order() {
        let first = RollId::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = RollId::new();
        assert!(first < second);
    }
}
