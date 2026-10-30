use serde::{Deserialize, Serialize};

use crate::ids::{FrameId, RollId};

/// How a frame's pixels were captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameKind {
    /// One RGB scan plus an infrared plane (film scanner).
    ScanRgbIr,
    /// Three exposures lit red, green and blue (camera copy stand).
    RgbTriplet,
}

/// One image position on a roll. The pixels live in source files; edits live in the Recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// RFC 3339 timestamps.
    pub created_at: String,
    pub edited_at: String,
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
            created_at: "2026-10-30T10:00:00Z".into(),
            edited_at: "2026-10-30T10:00:00Z".into(),
        };
        let again: Frame = serde_json::from_str(&serde_json::to_string(&frame).unwrap()).unwrap();
        assert_eq!(again, frame);
    }
}
