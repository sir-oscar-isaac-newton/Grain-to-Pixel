use serde::Deserialize;

use crate::ids::{RollId, TagId};

/// What to look for in the library. Every part is optional and all given parts must match.
///
/// - `text`: every word must match, as the start of a word, ignoring accents; it searches the
///   roll's name, film, camera, ISO, dates and notes, the frame note, and the names of the tags
///   (the frame's own and its roll's).
/// - `tags_all`: the frame must have every one of these tags, its own or from its roll.
/// - `film_stock`, `camera`: the roll's value, equal ignoring upper and lower case (plain letters).
/// - `iso`: the roll's box ISO or shot ISO.
/// - `date_from`, `date_to`: partial dates (`2026`, `2026-07`, `2026-07-14`). A roll matches when
///   its shooting dates overlap the range; a roll without dates never matches a date filter.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct SearchQuery {
    pub text: Option<String>,
    pub tags_all: Vec<TagId>,
    pub roll_id: Option<RollId>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub film_stock: Option<String>,
    pub camera: Option<String>,
    pub iso: Option<u32>,
}
