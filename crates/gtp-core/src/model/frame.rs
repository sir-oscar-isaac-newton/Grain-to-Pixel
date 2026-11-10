use serde::{Deserialize, Serialize};

use crate::ids::{FrameId, RollId};

/// How a frame's pixels were captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum FrameKind {
    /// One RGB scan plus an infrared plane (film scanner).
    ScanRgbIr,
    /// Three exposures lit red, green and blue (camera copy stand).
    RgbTriplet,
}

/// The pick or reject mark of a frame (culling after a scan).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum FrameFlag {
    /// Not judged yet.
    #[default]
    None,
    /// Worth editing and exporting.
    Pick,
    /// To be deleted.
    Reject,
}

impl FrameFlag {
    /// The text stored in the library (`"none"`, `"pick"` or `"reject"`).
    pub fn as_str(self) -> &'static str {
        match self {
            FrameFlag::None => "none",
            FrameFlag::Pick => "pick",
            FrameFlag::Reject => "reject",
        }
    }

    /// The flag stored as `text`, or `None` when the text is not one of the three.
    pub fn parse(text: &str) -> Option<FrameFlag> {
        match text {
            "none" => Some(FrameFlag::None),
            "pick" => Some(FrameFlag::Pick),
            "reject" => Some(FrameFlag::Reject),
            _ => None,
        }
    }
}

/// One image position on a roll. The pixels live in source files; edits live in the Recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Frame {
    pub id: FrameId,
    pub roll_id: RollId,
    pub index_in_roll: u32,
    pub kind: FrameKind,
    pub width: u32,
    pub height: u32,
    pub has_ir: bool,
    #[serde(default)]
    pub note: String,
    /// The pick or reject mark; not an edit of the picture, so it leaves `edited_at` alone.
    #[serde(default)]
    pub flag: FrameFlag,
    /// RFC 3339 timestamps; `edited_at` is `None` until the recipe is first saved.
    pub created_at: String,
    #[serde(default)]
    pub edited_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_kinds_use_their_names_in_json() {
        assert_eq!(
            serde_json::to_string(&FrameKind::ScanRgbIr).unwrap(),
            "\"ScanRgbIr\""
        );
        assert!(serde_json::from_str::<FrameKind>("\"Polaroid\"").is_err());
    }

    #[test]
    fn frame_flags_are_lowercase_words_in_json() {
        for (flag, text) in [
            (FrameFlag::None, "none"),
            (FrameFlag::Pick, "pick"),
            (FrameFlag::Reject, "reject"),
        ] {
            assert_eq!(serde_json::to_string(&flag).unwrap(), format!("\"{text}\""));
            assert_eq!(
                serde_json::from_str::<FrameFlag>(&format!("\"{text}\"")).unwrap(),
                flag
            );
            assert_eq!(FrameFlag::parse(text), Some(flag));
            assert_eq!(flag.as_str(), text);
        }
        assert_eq!(FrameFlag::default(), FrameFlag::None);
        assert!(serde_json::from_str::<FrameFlag>("\"Pick\"").is_err());
        assert_eq!(FrameFlag::parse("star"), None);
    }

    #[test]
    fn a_frame_round_trips() {
        let frame = Frame {
            id: FrameId::new(),
            roll_id: RollId::new(),
            index_in_roll: 3,
            kind: FrameKind::RgbTriplet,
            width: 6016,
            height: 4016,
            has_ir: false,
            note: String::new(),
            flag: FrameFlag::Pick,
            created_at: "2026-10-30T10:00:00Z".into(),
            edited_at: Some("2026-10-30T10:00:00Z".into()),
        };
        let again: Frame = serde_json::from_str(&serde_json::to_string(&frame).unwrap()).unwrap();
        assert_eq!(again, frame);
    }
}
