//! The flat field of a roll of camera triplets (P-21, decisions/0010-flat-field.md).
//!
//! The light that shines through the film is uneven across the frame, and differently for each of
//! the three exposures (red, green and blue LEDs). A capture of the bare light with the same
//! settings records that unevenness; dividing a frame by it takes it out. Only a small smooth grid
//! of it is kept: the light of each channel in each cell, 1 at the centre.

use serde::{Deserialize, Serialize};

use super::exposure::ExposureInfo;
use crate::ids::RollId;

/// The smoothed light of a bare-light triplet, for one roll.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlatField {
    /// Cells across and down the grid.
    pub grid_w: u32,
    pub grid_h: u32,
    /// The size in pixels of the frames it fits.
    pub frame_w: u32,
    pub frame_h: u32,
    /// Red, green and blue light of each cell, row by row, each channel divided by its value at the
    /// centre of the frame and never below [`FlatField::MIN_LIGHT`].
    pub light: Vec<f32>,
    /// The names of the three files it was made from, red, green, blue.
    pub sources: Vec<String>,
    /// How the three pictures were exposed, from their EXIF (H-24); empty when unknown (a flat field
    /// made before it was recorded, or from files without EXIF).
    #[serde(default)]
    pub exposure: ExposureInfo,
    /// The library-relative paths of the three NEF files kept for rebuilding it (the capture view
    /// keeps them); empty when none were kept (a flat field made from files that were only read).
    #[serde(default)]
    pub files: Vec<String>,
    /// RFC 3339 time the picture was shot, as the catalog stored it. `None` in a flat field not yet
    /// stored, which is stored with the time of storing; a copy of another roll's flat field (H-26)
    /// carries the source's, since it is the same picture.
    #[serde(default)]
    pub created_at: Option<String>,
}

/// A roll's flat field as the list of reusable ones shows it (H-26).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FlatFieldSummary {
    pub roll_id: RollId,
    pub roll_name: String,
    /// RFC 3339 time the flat field was made.
    pub created_at: String,
    /// How the three pictures were exposed; empty fields are unknown.
    pub exposure: ExposureInfo,
}

/// What the UI is told of a roll's flat field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FlatFieldInfo {
    /// The size in pixels of the frames it fits.
    pub frame_width: u32,
    pub frame_height: u32,
    /// The names of the three files it was made from.
    pub sources: Vec<String>,
    /// How the three pictures were exposed (H-24); empty fields are unknown.
    pub exposure: ExposureInfo,
}

impl From<&FlatField> for FlatFieldInfo {
    fn from(flat: &FlatField) -> Self {
        Self {
            frame_width: flat.frame_w,
            frame_height: flat.frame_h,
            sources: flat.sources.clone(),
            exposure: flat.exposure,
        }
    }
}

impl FlatField {
    /// The least light a cell may have, so that the division stays finite where the carrier is
    /// almost black.
    pub const MIN_LIGHT: f32 = 0.05;

    /// Whether the grid has the cells it says it has and every value is usable.
    pub fn is_valid(&self) -> bool {
        self.grid_w >= 2
            && self.grid_h >= 2
            && self.light.len() == (self.grid_w as usize) * (self.grid_h as usize) * 3
            && self
                .light
                .iter()
                .all(|v| v.is_finite() && *v >= Self::MIN_LIGHT)
            && self.frame_w > 0
            && self.frame_h > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(light: Vec<f32>) -> FlatField {
        FlatField {
            grid_w: 2,
            grid_h: 2,
            frame_w: 100,
            frame_h: 80,
            light,
            sources: vec!["r".into(), "g".into(), "b".into()],
            exposure: ExposureInfo::default(),
            files: Vec::new(),
            created_at: None,
        }
    }

    #[test]
    fn a_flat_field_needs_all_its_cells_and_usable_values() {
        assert!(grid(vec![1.0; 12]).is_valid());
        assert!(!grid(vec![1.0; 11]).is_valid(), "a cell is missing");
        let mut low = vec![1.0; 12];
        low[3] = 0.01;
        assert!(!grid(low).is_valid(), "below the floor");
        let mut nan = vec![1.0; 12];
        nan[0] = f32::NAN;
        assert!(!grid(nan).is_valid());
        let mut tiny = grid(vec![1.0; 12]);
        tiny.grid_w = 1;
        assert!(!tiny.is_valid());
        let json = serde_json::to_string(&grid(vec![1.0; 12])).unwrap();
        assert_eq!(
            serde_json::from_str::<FlatField>(&json).unwrap(),
            grid(vec![1.0; 12])
        );
    }
}
