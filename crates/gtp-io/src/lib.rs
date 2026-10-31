//! File formats: 16-bit TIFF (docs/specification.md sections 5.1 and 9), hashing of originals,
//! and colour-managed export (section 4.2 step 9). RAW decoding arrives with a later issue.

mod export;
mod hash;
mod icc;
mod limits;
mod tiff;

pub use export::{encode_jpeg, encode_tiff16, write_output_tiff16, JPEG_QUALITY};
pub use hash::{mark_read_only, sha256_file, sha256_file_with};
pub use icc::{icc_profile, to_output, Tiff16Output};
pub use limits::Limits;
pub use tiff::{
    from_f32_linear, inspect_tiff, merge_rgbi, read_tiff16, split_rgbi, to_f32_linear,
    write_tiff16, Compression, Tiff16, TiffLayout,
};
