use serde::{Deserialize, Serialize};

use crate::error::{AppError, ErrorCode};
use crate::ids::TagId;

const MAX_TAG_CHARS: usize = 64;

/// A free-form label on frames and rolls. The name keeps the spelling it was first typed with;
/// two names are the same tag when their [`Tag::key`]s are equal (case-insensitive across Unicode).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub id: TagId,
    pub name: String,
}

impl Tag {
    /// A new tag with a normalized name.
    pub fn new(name: &str) -> Result<Self, AppError> {
        Ok(Self {
            id: TagId::new(),
            name: Self::normalize(name)?,
        })
    }

    /// The comparison key of a name: normalized, then lower-cased across Unicode
    /// (`" ÉTÉ  à Paris"` and `"été À PARIS"` share `"été à paris"`). Accents still count.
    pub fn key(name: &str) -> Result<String, AppError> {
        Ok(Self::normalize(name)?.to_lowercase())
    }

    /// Trims and collapses inner whitespace, keeping the case; rejects empty or very long names.
    pub fn normalize(name: &str) -> Result<String, AppError> {
        let normalized = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "A tag needs a name.",
            ));
        }
        if normalized.chars().count() > MAX_TAG_CHARS {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "That tag name is too long.",
            ));
        }
        Ok(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_collapsed_but_keep_their_case() {
        assert_eq!(
            Tag::normalize("  Kodak   PORTRA \t400 ").unwrap(),
            "Kodak PORTRA 400"
        );
        assert_eq!(Tag::new("Family").unwrap().name, "Family");
    }

    #[test]
    fn keys_ignore_case_across_unicode_but_not_accents() {
        assert_eq!(Tag::key(" ÉTÉ  à Paris").unwrap(), "été à paris");
        assert_eq!(Tag::key("Ålesund").unwrap(), Tag::key("ÅLESUND").unwrap());
        assert_eq!(Tag::key("München").unwrap(), Tag::key("MÜNCHEN").unwrap());
        assert_ne!(Tag::key("Ete").unwrap(), Tag::key("Été").unwrap());
    }

    #[test]
    fn empty_and_overlong_names_are_rejected() {
        assert_eq!(
            Tag::normalize("  \n ").unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let long = "x".repeat(MAX_TAG_CHARS + 1);
        assert_eq!(
            Tag::normalize(&long).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        assert!(Tag::normalize(&"x".repeat(MAX_TAG_CHARS)).is_ok());
    }
}
