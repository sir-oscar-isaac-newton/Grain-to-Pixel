//! Quality harness (Q-01): the CIEDE2000 colour difference, image comparison, and golden-image
//! regression tests of the whole pipeline; and (Q-02) the report against reference renders. The command-line front end is `src/main.rs`.

pub mod delta_e;
pub mod diff;
pub mod fit;
pub mod fixture;
pub mod golden;
pub mod manifest;
pub mod register;
pub mod report;

pub use delta_e::{delta_e2000, rec2020_to_lab};
pub use diff::{delta_e_map, ImageDiff};
pub use golden::{bless, check, Case, Outcome, CASES, GOLDEN_SCALE, MAX_DELTA_E};
