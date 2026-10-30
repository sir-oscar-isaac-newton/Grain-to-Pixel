//! File formats: 16-bit TIFF (docs/specification.md sections 5.1 and 9), hashing of originals.
//! Colour-managed export encoders and RAW decoding arrive with later issues.

mod hash;
mod limits;
mod tiff;

pub use hash::{mark_read_only, sha256_file};
pub use limits::Limits;
pub use tiff::{
    from_f32_linear, inspect_tiff, merge_rgbi, read_tiff16, split_rgbi, to_f32_linear,
    write_tiff16, Compression, Tiff16, TiffLayout,
};
