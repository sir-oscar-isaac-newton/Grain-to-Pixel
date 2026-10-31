//! Quality harness (Q-01): the CIEDE2000 colour difference, image comparison, and golden-image
//! regression tests of the whole pipeline. The command-line front end is `src/main.rs`.

pub mod delta_e;
pub mod diff;
pub mod fixture;
pub mod golden;

pub use delta_e::{delta_e2000, rec2020_to_lab};
pub use diff::{delta_e_map, ImageDiff};
pub use golden::{bless, check, Case, Outcome, CASES, GOLDEN_SCALE, MAX_DELTA_E};
