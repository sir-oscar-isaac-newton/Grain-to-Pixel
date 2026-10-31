//! Export encoders: a converted image ([`to_output`]) written as a 16-bit TIFF or an 8-bit JPEG,
//! each with its ICC profile embedded. Both write through a temporary file and a rename.

use std::borrow::Cow;
use std::io::{Seek, Write};
use std::path::Path;

use gtp_core::recipe::OutputSpace;
use gtp_core::{AppError, ErrorCode, ImageF32};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use tiff::encoder::colortype::RGB16;
use tiff::encoder::compression::DeflateLevel;
use tiff::encoder::{Compression as TiffCompression, TiffEncoder, TiffValue};
use tiff::tags::{Predictor, Tag, Type};

use crate::icc::{to_output, Tiff16Output};
use crate::tiff::write_through_temp;

/// The lowest and highest JPEG quality.
pub const JPEG_QUALITY: std::ops::RangeInclusive<u8> = 1..=100;

/// The ICC profile as a TIFF value of type UNDEFINED, as the TIFF/ICC specifications ask.
struct IccValue<'a>(&'a [u8]);

impl TiffValue for IccValue<'_> {
    const BYTE_LEN: u8 = 1;
    const FIELD_TYPE: Type = Type::UNDEFINED;

    fn count(&self) -> usize {
        self.0.len()
    }

    fn data(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(self.0)
    }
}

fn encode_failed(message: &str, err: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Io, message).with_details(err.to_string())
}

fn encode_tiff(writer: impl Write + Seek, image: &Tiff16Output) -> tiff::TiffResult<()> {
    let mut encoder = TiffEncoder::new(writer)?
        .with_compression(TiffCompression::Deflate(DeflateLevel::Balanced))
        .with_predictor(Predictor::Horizontal);
    let mut tiff = encoder.new_image::<RGB16>(image.width, image.height)?;
    tiff.encoder()
        .write_tag(Tag::IccProfile, IccValue(&image.icc))?;
    tiff.write_data(&image.data)
}

/// Writes an already converted image as a Deflate-compressed 16-bit RGB TIFF with its profile.
pub fn write_output_tiff16(path: &Path, image: &Tiff16Output) -> Result<(), AppError> {
    write_through_temp(path, |writer| {
        encode_tiff(writer, image)
            .map_err(|err| encode_failed("The TIFF file could not be written.", err))
    })
}

/// Converts a linear Rec.2020 image to `space` and writes it as a 16-bit TIFF.
pub fn encode_tiff16(image: &ImageF32, space: OutputSpace, path: &Path) -> Result<(), AppError> {
    write_output_tiff16(path, &to_output(image, space)?)
}

/// Converts a linear Rec.2020 image to `space` and writes it as a baseline JPEG of the given
/// quality (1 to 100), without chroma subsampling.
pub fn encode_jpeg(
    image: &ImageF32,
    space: OutputSpace,
    quality: u8,
    path: &Path,
) -> Result<(), AppError> {
    if !JPEG_QUALITY.contains(&quality) {
        return Err(
            AppError::new(ErrorCode::InvalidInput, "JPEG quality runs from 1 to 100.")
                .with_details(format!("{quality}")),
        );
    }
    let too_large = || {
        AppError::new(
            ErrorCode::InvalidInput,
            "This image is too large for a JPEG file.",
        )
        .with_details(format!("{}x{}", image.width, image.height))
    };
    let width = u16::try_from(image.width).map_err(|_| too_large())?;
    let height = u16::try_from(image.height).map_err(|_| too_large())?;

    let output = to_output(image, space)?;
    // 16 to 8 bits on the same curve: round(v / 257).
    let rgb8: Vec<u8> = output
        .data
        .iter()
        .map(|&v| ((u32::from(v) + 128) / 257) as u8)
        .collect();

    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, quality);
    encoder.set_sampling_factor(SamplingFactor::F_1_1);
    encoder
        .add_icc_profile(&output.icc)
        .map_err(|err| encode_failed("The JPEG file could not be written.", err))?;
    encoder
        .encode(&rgb8, width, height, ColorType::Rgb)
        .map_err(|err| encode_failed("The JPEG file could not be written.", err))?;

    write_through_temp(path, |writer| {
        writer
            .write_all(&bytes)
            .map_err(|err| AppError::from_io(&err))
    })
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::path::PathBuf;

    use lcms2::{ColorSpaceSignature, Profile};
    use tiff::decoder::Decoder;

    use super::*;
    use crate::{read_tiff16, Limits};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("gtp-export-{label}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A smooth colourful test picture with some fine detail, in linear Rec.2020.
    fn picture(width: u32, height: u32) -> ImageF32 {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                let (u, v) = (x as f32 / width as f32, y as f32 / height as f32);
                let detail = if (x / 3 + y / 3) % 2 == 0 { 0.05 } else { 0.0 };
                data.extend_from_slice(&[u * 0.9 + detail, v * 0.8, (1.0 - u) * 0.6 + detail]);
            }
        }
        ImageF32::new(width, height, 3, data).unwrap()
    }

    /// The ICC profile in a JPEG's APP2 "ICC_PROFILE" segments, joined in order.
    fn jpeg_icc(bytes: &[u8]) -> Vec<u8> {
        let mut chunks = Vec::new();
        let mut i = 2;
        while i + 4 <= bytes.len() && bytes[i] == 0xFF {
            let marker = bytes[i + 1];
            let len = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
            let body = &bytes[i + 4..i + 2 + len];
            if marker == 0xE2 && body.starts_with(b"ICC_PROFILE\0") {
                chunks.push((body[12], body[14..].to_vec()));
            }
            if marker == 0xDA {
                break;
            }
            i += 2 + len;
        }
        chunks.sort_by_key(|(n, _)| *n);
        chunks.into_iter().flat_map(|(_, c)| c).collect()
    }

    #[test]
    fn a_tiff_export_reads_back_with_its_profile() {
        let dir = TempDir::new("tiff");
        let img = picture(64, 40);
        for space in [
            OutputSpace::Srgb,
            OutputSpace::AdobeRgb,
            OutputSpace::Prophoto,
        ] {
            let path = dir.0.join(format!("{space:?}.tif"));
            encode_tiff16(&img, space, &path).unwrap();

            let back = read_tiff16(&path, &Limits::default()).unwrap();
            let expected = to_output(&img, space).unwrap();
            assert_eq!((back.width, back.height, back.channels), (64, 40, 3));
            assert_eq!(back.data, expected.data, "{space:?} pixels are lossless");

            let mut decoder = Decoder::new(File::open(&path).unwrap()).unwrap();
            let icc = decoder.get_tag_u8_vec(Tag::IccProfile).unwrap();
            assert_eq!(icc, expected.icc, "{space:?}");
            let parsed = Profile::new_icc(&icc).unwrap();
            assert_eq!(parsed.color_space(), ColorSpaceSignature::RgbData);
        }
    }

    #[test]
    fn a_jpeg_export_carries_its_profile() {
        let dir = TempDir::new("jpeg");
        let path = dir.0.join("frame.jpg");
        encode_jpeg(&picture(64, 40), OutputSpace::AdobeRgb, 90, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "a JPEG file");
        let icc = jpeg_icc(&bytes);
        assert_eq!(icc, crate::icc_profile(OutputSpace::AdobeRgb).unwrap());
    }

    #[test]
    fn jpeg_size_grows_with_quality() {
        let dir = TempDir::new("quality");
        let img = picture(160, 120);
        let sizes: Vec<u64> = [10, 40, 70, 90, 100]
            .iter()
            .map(|&q| {
                let path = dir.0.join(format!("q{q}.jpg"));
                encode_jpeg(&img, OutputSpace::Srgb, q, &path).unwrap();
                std::fs::metadata(&path).unwrap().len()
            })
            .collect();
        assert!(sizes.windows(2).all(|w| w[0] < w[1]), "{sizes:?}");
    }

    #[test]
    fn bad_quality_size_and_channels_are_rejected() {
        let dir = TempDir::new("errors");
        let path = dir.0.join("x.jpg");
        let img = picture(8, 8);
        for q in [0, 101] {
            let err = encode_jpeg(&img, OutputSpace::Srgb, q, &path).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
        }
        let wide = ImageF32::zeros(70_000, 1, 3).unwrap();
        let err = encode_jpeg(&wide, OutputSpace::Srgb, 90, &path).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let gray = ImageF32::zeros(4, 4, 1).unwrap();
        let err = encode_tiff16(&gray, OutputSpace::Srgb, &dir.0.join("g.tif")).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(!path.exists(), "nothing is left behind");
    }

    #[test]
    fn a_missing_folder_is_reported() {
        let path = std::env::temp_dir()
            .join("gtp-no-such-folder-xyz")
            .join("a.tif");
        let err = encode_tiff16(&picture(4, 4), OutputSpace::Srgb, &path).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    /// Export of a 21 MP frame; run with
    /// `cargo test -p gtp-io --release -- --ignored --nocapture` to see the times.
    #[test]
    #[ignore = "timing run, not a check"]
    fn time_export_of_a_21_megapixel_frame() {
        let dir = TempDir::new("timing");
        let img = picture(5616, 3744);
        let start = std::time::Instant::now();
        let out = to_output(&img, OutputSpace::AdobeRgb).unwrap();
        eprintln!("transform: {:?}", start.elapsed());
        let start = std::time::Instant::now();
        write_output_tiff16(&dir.0.join("big.tif"), &out).unwrap();
        eprintln!("TIFF write: {:?}", start.elapsed());
        let start = std::time::Instant::now();
        encode_jpeg(&img, OutputSpace::Srgb, 90, &dir.0.join("big.jpg")).unwrap();
        eprintln!("JPEG (transform + encode): {:?}", start.elapsed());
    }
}
