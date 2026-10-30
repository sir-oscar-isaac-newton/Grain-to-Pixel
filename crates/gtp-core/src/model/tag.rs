use serde::{Deserialize, Serialize};

use crate::error::{AppError, ErrorCode};
use crate::ids::TagId;

const MAX_TAG_CHARS: usize = 64;

/// A free-form label on frames. Names are unique case-insensitively, so they are stored normalized.
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

    /// Trims, collapses inner whitespace and lower-cases; rejects empty or very long names.
    pub fn normalize(name: &str) -> Result<String, AppError> {
        let normalized = name
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
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
    fn names_are_trimmed_collapsed_and_lower_cased() {
        assert_eq!(
            Tag::normalize("  Kodak   PORTRA \t400 ").unwrap(),
            "kodak portra 400"
        );
        assert_eq!(Tag::new("Family").unwrap().name, "family");
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
