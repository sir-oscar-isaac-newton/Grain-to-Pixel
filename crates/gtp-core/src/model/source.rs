use serde::{Deserialize, Serialize};

use crate::ids::FrameId;

/// Which plane or exposure a source file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum SourceRole {
    Rgb,
    Ir,
    R,
    G,
    B,
}

/// An original file of a frame. Originals are immutable; `sha256` detects changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SourceFile {
    pub frame_id: FrameId,
    pub role: SourceRole,
    /// Path relative to the library folder.
    pub rel_path: String,
    pub sha256: String,
    /// A JavaScript number on the IPC boundary (exact up to 9 PB).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_written_in_lower_case() {
        for (role, text) in [
            (SourceRole::Rgb, "rgb"),
            (SourceRole::Ir, "ir"),
            (SourceRole::R, "r"),
            (SourceRole::G, "g"),
            (SourceRole::B, "b"),
        ] {
            assert_eq!(serde_json::to_string(&role).unwrap(), format!("\"{text}\""));
        }
    }
}
