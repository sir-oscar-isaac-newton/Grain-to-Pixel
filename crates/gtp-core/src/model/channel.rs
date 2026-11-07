//! Which light an exposure of an RGB triplet was taken in.

use serde::{Deserialize, Serialize};

/// The light an exposure was taken in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum TripletChannel {
    Red,
    Green,
    Blue,
}

impl TripletChannel {
    pub const ALL: [TripletChannel; 3] = [
        TripletChannel::Red,
        TripletChannel::Green,
        TripletChannel::Blue,
    ];

    /// 0 for red, 1 for green, 2 for blue: the position in a triplet.
    pub fn index(self) -> usize {
        match self {
            TripletChannel::Red => 0,
            TripletChannel::Green => 1,
            TripletChannel::Blue => 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_know_their_place_in_a_triplet() {
        assert_eq!(TripletChannel::ALL.map(TripletChannel::index), [0, 1, 2]);
    }
}
