//! The flat field of a roll of camera triplets (P-21, decisions/0010-flat-field.md).
//!
//! The light that shines through the film is uneven across the frame, and differently for each of
//! the three exposures (red, green and blue LEDs). A capture of the bare light with the same
//! settings records that unevenness; dividing a frame by it takes it out. Only a small smooth grid
//! of it is kept: the light of each channel in each cell, 1 at the centre.

use serde::{Deserialize, Serialize};

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
