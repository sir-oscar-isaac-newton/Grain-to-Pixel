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
use crate::metadata::{jpeg_exif_block, ExportMetadata, Fields, EXIF_VERSION};
use crate::tiff::write_through_temp;

const TAG_EXIF_VERSION: u16 = 0x9000;
const TAG_PHOTOGRAPHIC_SENSITIVITY: u16 = 0x8827;
const TAG_DATE_TIME_ORIGINAL: u16 = 0x9003;
const TAG_USER_COMMENT: u16 = 0x9286;

/// The lowest and highest JPEG quality.
pub const JPEG_QUALITY: std::ops::RangeInclusive<u8> = 1..=100;

/// Bytes as a TIFF value of type UNDEFINED: the ICC profile, as the TIFF/ICC specifications ask,
/// and the Exif version and user comment.
struct UndefinedValue<'a>(&'a [u8]);

impl TiffValue for UndefinedValue<'_> {
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

/// Writes the Exif tags (ISO, date, comment) as a directory of their own and returns where it
/// starts; the image directory points to it with the ExifIFD tag.
fn write_exif_directory<W: Write + Seek>(
    encoder: &mut TiffEncoder<W>,
    fields: &Fields,
) -> tiff::TiffResult<u32> {
    let mut dir = encoder.extra_directory()?;
    dir.write_tag(Tag::Unknown(TAG_EXIF_VERSION), UndefinedValue(EXIF_VERSION))?;
    if let Some(iso) = fields.iso {
        dir.write_tag(Tag::Unknown(TAG_PHOTOGRAPHIC_SENSITIVITY), iso)?;
    }
    if let Some(date) = &fields.date_original {
        dir.write_tag(Tag::Unknown(TAG_DATE_TIME_ORIGINAL), date.as_str())?;
    }
    // The file is little-endian, so the UTF-16 comment is too.
    if let Some(comment) = fields.comment_bytes(false) {
        dir.write_tag(Tag::Unknown(TAG_USER_COMMENT), UndefinedValue(&comment))?;
    }
    Ok(dir.finish_with_offsets()?.offset)
}

fn encode_tiff(
    writer: impl Write + Seek,
    image: &Tiff16Output,
    metadata: Option<&ExportMetadata>,
) -> tiff::TiffResult<()> {
    let fields = metadata
        .map(ExportMetadata::fields)
        .filter(|f| !f.is_empty());
    let mut encoder = TiffEncoder::new(writer)?
        .with_compression(TiffCompression::Deflate(DeflateLevel::Balanced))
        .with_predictor(Predictor::Horizontal);
    let exif_offset = match &fields {
        Some(fields) if fields.has_exif_tags() => Some(write_exif_directory(&mut encoder, fields)?),
        _ => None,
    };
    let mut tiff = encoder.new_image::<RGB16>(image.width, image.height)?;
    let dir = tiff.encoder();
    dir.write_tag(Tag::IccProfile, UndefinedValue(&image.icc))?;
    if let Some(fields) = &fields {
        for (tag, text) in [
            (Tag::ImageDescription, &fields.description),
            (Tag::Make, &fields.make),
            (Tag::Model, &fields.model),
            (Tag::Software, &fields.software),
        ] {
            if let Some(text) = text {
                dir.write_tag(tag, text.as_str())?;
            }
        }
    }
    if let Some(offset) = exif_offset {
        // Type LONG, as Exif asks (the tiff crate's IFD type is not read as a pointer by Exif readers).
        dir.write_tag(Tag::ExifDirectory, offset)?;
    }
    tiff.write_data(&image.data)
}

/// Writes an already converted image as a Deflate-compressed 16-bit RGB TIFF with its profile, and
/// the metadata when given (none at all otherwise).
pub fn write_output_tiff16(
    path: &Path,
    image: &Tiff16Output,
    metadata: Option<&ExportMetadata>,
) -> Result<(), AppError> {
    write_through_temp(path, |writer| {
        encode_tiff(writer, image, metadata)
            .map_err(|err| encode_failed("The TIFF file could not be written.", err))
    })
}

/// Converts a linear Rec.2020 image to `space` and writes it as a 16-bit TIFF, with `metadata`
/// when given.
pub fn encode_tiff16(
    image: &ImageF32,
    space: OutputSpace,
    path: &Path,
    metadata: Option<&ExportMetadata>,
) -> Result<(), AppError> {
    write_output_tiff16(path, &to_output(image, space)?, metadata)
}

/// Converts a linear Rec.2020 image to `space` and writes it as a baseline JPEG of the given
/// quality (1 to 100), without chroma subsampling, with `metadata` when given.
pub fn encode_jpeg(
    image: &ImageF32,
    space: OutputSpace,
    quality: u8,
    path: &Path,
    metadata: Option<&ExportMetadata>,
) -> Result<(), AppError> {
    let bytes = encode_jpeg_bytes(image, space, quality, metadata)?;
    write_through_temp(path, |writer| {
        writer
            .write_all(&bytes)
            .map_err(|err| AppError::from_io(&err))
    })
}

/// [`encode_jpeg`] to memory.
pub fn encode_jpeg_bytes(
    image: &ImageF32,
    space: OutputSpace,
    quality: u8,
    metadata: Option<&ExportMetadata>,
) -> Result<Vec<u8>, AppError> {
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
    if let Some(block) = metadata.map(jpeg_exif_block).transpose()?.flatten() {
        encoder
            .add_exif_metadata(&block)
            .map_err(|err| encode_failed("The JPEG file could not be written.", err))?;
    }
    encoder
        .encode(&rgb8, width, height, ColorType::Rgb)
        .map_err(|err| encode_failed("The JPEG file could not be written.", err))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::path::PathBuf;

    use lcms2::{ColorSpaceSignature, Profile};
    use tiff::decoder::Decoder;

    use super::*;
    use crate::{read_tiff16, ExifDate, Limits};

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

    /// A profile with its header creation date (bytes 24 to 35) blanked: Little CMS stamps the
    /// time a profile is built, so two builds a second apart differ only there.
    fn without_date(icc: &[u8]) -> Vec<u8> {
        let mut icc = icc.to_vec();
        icc[24..36].fill(0);
        icc
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
            encode_tiff16(&img, space, &path, None).unwrap();

            let back = read_tiff16(&path, &Limits::default()).unwrap();
            let expected = to_output(&img, space).unwrap();
            assert_eq!((back.width, back.height, back.channels), (64, 40, 3));
            assert_eq!(back.data, expected.data, "{space:?} pixels are lossless");

            let mut decoder = Decoder::new(File::open(&path).unwrap()).unwrap();
            let icc = decoder.get_tag_u8_vec(Tag::IccProfile).unwrap();
            assert_eq!(without_date(&icc), without_date(&expected.icc), "{space:?}");
            let parsed = Profile::new_icc(&icc).unwrap();
            assert_eq!(parsed.color_space(), ColorSpaceSignature::RgbData);
        }
    }

    #[test]
    fn a_jpeg_export_carries_its_profile() {
        let dir = TempDir::new("jpeg");
        let path = dir.0.join("frame.jpg");
        encode_jpeg(&picture(64, 40), OutputSpace::AdobeRgb, 90, &path, None).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "a JPEG file");
        let icc = jpeg_icc(&bytes);
        assert_eq!(
            without_date(&icc),
            without_date(&crate::icc_profile(OutputSpace::AdobeRgb).unwrap())
        );
    }

    fn sample_metadata() -> ExportMetadata {
        ExportMetadata {
            software: Some("Grain to Pixel 1.2.3".into()),
            description: Some("Kodak Portra 400 (box ISO 400), Roll 12".into()),
            make: Some("Nikon".into()),
            model: Some("Nikon F3".into()),
            iso: Some(800),
            date_original: Some(ExifDate {
                year: 2026,
                month: 7,
                day: 14,
            }),
            user_comment: Some("Harbour at dusk".into()),
        }
    }

    fn read_exif(path: &Path) -> exif::Exif {
        let mut reader = std::io::BufReader::new(File::open(path).unwrap());
        exif::Reader::new()
            .read_from_container(&mut reader)
            .unwrap()
    }

    fn text(exif: &exif::Exif, tag: exif::Tag) -> Option<String> {
        match &exif.get_field(tag, exif::In::PRIMARY)?.value {
            exif::Value::Ascii(parts) => Some(String::from_utf8_lossy(&parts[0]).into_owned()),
            _ => None,
        }
    }

    fn comment(exif: &exif::Exif) -> Option<Vec<u8>> {
        match &exif
            .get_field(exif::Tag::UserComment, exif::In::PRIMARY)?
            .value
        {
            exif::Value::Undefined(bytes, _) => Some(bytes.clone()),
            _ => None,
        }
    }

    fn iso(exif: &exif::Exif) -> Option<u32> {
        exif.get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY)?
            .value
            .get_uint(0)
    }

    const ALL_TAGS: [exif::Tag; 7] = [
        exif::Tag::Software,
        exif::Tag::ImageDescription,
        exif::Tag::Make,
        exif::Tag::Model,
        exif::Tag::PhotographicSensitivity,
        exif::Tag::DateTimeOriginal,
        exif::Tag::UserComment,
    ];

    fn assert_sample_tags(exif: &exif::Exif) {
        assert_eq!(
            text(exif, exif::Tag::Software).unwrap(),
            "Grain to Pixel 1.2.3"
        );
        assert_eq!(
            text(exif, exif::Tag::ImageDescription).unwrap(),
            "Kodak Portra 400 (box ISO 400), Roll 12"
        );
        assert_eq!(text(exif, exif::Tag::Make).unwrap(), "Nikon");
        assert_eq!(text(exif, exif::Tag::Model).unwrap(), "Nikon F3");
        assert_eq!(iso(exif), Some(800));
        assert_eq!(
            text(exif, exif::Tag::DateTimeOriginal).unwrap(),
            "2026:07:14 00:00:00"
        );
        assert_eq!(comment(exif).unwrap(), b"ASCII\0\0\0Harbour at dusk");
    }

    /// The UTF-16 text of a `UNICODE\0` comment in the given byte order.
    fn unicode_comment(bytes: &[u8], big_endian: bool) -> String {
        assert!(bytes.starts_with(b"UNICODE\0"));
        let (pairs, rest) = bytes[8..].as_chunks::<2>();
        assert!(rest.is_empty());
        let units: Vec<u16> = pairs
            .iter()
            .map(|&c| {
                if big_endian {
                    u16::from_be_bytes(c)
                } else {
                    u16::from_le_bytes(c)
                }
            })
            .collect();
        String::from_utf16(&units).unwrap()
    }

    /// The bytes from the start of the scan on: the compressed picture.
    fn jpeg_scan(bytes: &[u8]) -> &[u8] {
        let mut i = 2;
        while bytes[i + 1] != 0xDA {
            i += 2 + usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
        }
        &bytes[i..]
    }

    #[test]
    fn a_jpeg_export_carries_its_metadata() {
        let dir = TempDir::new("jpeg-meta");
        let path = dir.0.join("m.jpg");
        let meta = sample_metadata();
        encode_jpeg(&picture(64, 40), OutputSpace::Srgb, 90, &path, Some(&meta)).unwrap();
        assert_sample_tags(&read_exif(&path));
    }

    #[test]
    fn a_tiff_export_carries_its_metadata() {
        let dir = TempDir::new("tiff-meta");
        let path = dir.0.join("m.tif");
        let meta = sample_metadata();
        encode_tiff16(&picture(64, 40), OutputSpace::Srgb, &path, Some(&meta)).unwrap();
        assert_sample_tags(&read_exif(&path));
        // The project's own reader still reads it.
        let back = read_tiff16(&path, &Limits::default()).unwrap();
        assert_eq!((back.width, back.height, back.channels), (64, 40, 3));
    }

    #[test]
    fn without_metadata_there_are_no_tags() {
        let dir = TempDir::new("no-meta");
        let img = picture(32, 20);
        let jpg = dir.0.join("n.jpg");
        encode_jpeg(&img, OutputSpace::Srgb, 90, &jpg, None).unwrap();
        let bytes = std::fs::read(&jpg).unwrap();
        assert!(
            !bytes.windows(6).any(|w| w == b"Exif\0\0"),
            "no Exif block at all"
        );
        let tif = dir.0.join("n.tif");
        encode_tiff16(&img, OutputSpace::Srgb, &tif, None).unwrap();
        let exif = read_exif(&tif);
        for tag in ALL_TAGS {
            assert!(exif.get_field(tag, exif::In::PRIMARY).is_none(), "{tag}");
        }
        // Metadata with nothing in it writes nothing either.
        let empty = ExportMetadata::default();
        let jpg2 = dir.0.join("e.jpg");
        encode_jpeg(&img, OutputSpace::Srgb, 90, &jpg2, Some(&empty)).unwrap();
        assert_eq!(std::fs::read(&jpg2).unwrap(), bytes);
        let tif2 = dir.0.join("e.tif");
        encode_tiff16(&img, OutputSpace::Srgb, &tif2, Some(&empty)).unwrap();
        let exif = read_exif(&tif2);
        for tag in ALL_TAGS {
            assert!(exif.get_field(tag, exif::In::PRIMARY).is_none(), "{tag}");
        }
    }

    #[test]
    fn a_note_with_other_scripts_survives_and_ascii_fields_get_question_marks() {
        let dir = TempDir::new("unicode");
        let note = "Café at the pier\n\nさようなら ☃";
        let meta = ExportMetadata {
            software: Some("Grain to Pixel".into()),
            description: Some("Pellicule Kodak é, Roll ☃".into()),
            make: Some("Cañon".into()),
            model: Some("   ".into()),
            user_comment: Some(note.into()),
            ..ExportMetadata::default()
        };
        let img = picture(16, 16);
        let jpg = dir.0.join("u.jpg");
        let tif = dir.0.join("u.tif");
        encode_jpeg(&img, OutputSpace::Srgb, 90, &jpg, Some(&meta)).unwrap();
        encode_tiff16(&img, OutputSpace::Srgb, &tif, Some(&meta)).unwrap();
        for (path, big_endian) in [(&jpg, true), (&tif, false)] {
            let exif = read_exif(path);
            assert_eq!(
                text(&exif, exif::Tag::ImageDescription).unwrap(),
                "Pellicule Kodak ?, Roll ?"
            );
            assert_eq!(text(&exif, exif::Tag::Make).unwrap(), "Ca?on");
            assert!(exif
                .get_field(exif::Tag::Model, exif::In::PRIMARY)
                .is_none());
            assert!(iso(&exif).is_none());
            assert!(exif
                .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
                .is_none());
            let bytes = comment(&exif).unwrap();
            assert_eq!(unicode_comment(&bytes, big_endian), note);
        }
    }

    #[test]
    fn iso_above_the_16_bit_limit_is_left_out() {
        let dir = TempDir::new("iso");
        let meta = ExportMetadata {
            iso: Some(70_000),
            software: Some("x".into()),
            ..ExportMetadata::default()
        };
        let tif = dir.0.join("i.tif");
        encode_tiff16(&picture(8, 8), OutputSpace::Srgb, &tif, Some(&meta)).unwrap();
        let exif = read_exif(&tif);
        assert!(iso(&exif).is_none());
        assert_eq!(text(&exif, exif::Tag::Software).unwrap(), "x");
    }

    #[test]
    fn metadata_leaves_pixels_and_profile_alone() {
        let dir = TempDir::new("meta-pixels");
        let img = picture(64, 40);
        let meta = sample_metadata();

        let (plain, tagged) = (dir.0.join("p.tif"), dir.0.join("t.tif"));
        encode_tiff16(&img, OutputSpace::AdobeRgb, &plain, None).unwrap();
        encode_tiff16(&img, OutputSpace::AdobeRgb, &tagged, Some(&meta)).unwrap();
        let (a, b) = (
            read_tiff16(&plain, &Limits::default()).unwrap(),
            read_tiff16(&tagged, &Limits::default()).unwrap(),
        );
        assert_eq!(a.data, b.data);
        let icc = |p: &Path| {
            Decoder::new(File::open(p).unwrap())
                .unwrap()
                .get_tag_u8_vec(Tag::IccProfile)
                .unwrap()
        };
        assert_eq!(without_date(&icc(&plain)), without_date(&icc(&tagged)));

        let plain = encode_jpeg_bytes(&img, OutputSpace::AdobeRgb, 90, None).unwrap();
        let tagged = encode_jpeg_bytes(&img, OutputSpace::AdobeRgb, 90, Some(&meta)).unwrap();
        assert_eq!(jpeg_scan(&plain), jpeg_scan(&tagged), "same picture data");
        assert_eq!(
            without_date(&jpeg_icc(&plain)),
            without_date(&jpeg_icc(&tagged))
        );
    }

    #[test]
    fn jpeg_size_grows_with_quality() {
        let dir = TempDir::new("quality");
        let img = picture(160, 120);
        let sizes: Vec<u64> = [10, 40, 70, 90, 100]
            .iter()
            .map(|&q| {
                let path = dir.0.join(format!("q{q}.jpg"));
                encode_jpeg(&img, OutputSpace::Srgb, q, &path, None).unwrap();
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
            let err = encode_jpeg(&img, OutputSpace::Srgb, q, &path, None).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
        }
        let wide = ImageF32::zeros(70_000, 1, 3).unwrap();
        let err = encode_jpeg(&wide, OutputSpace::Srgb, 90, &path, None).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let gray = ImageF32::zeros(4, 4, 1).unwrap();
        let err = encode_tiff16(&gray, OutputSpace::Srgb, &dir.0.join("g.tif"), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(!path.exists(), "nothing is left behind");
    }

    #[test]
    fn a_missing_folder_is_reported() {
        let path = std::env::temp_dir()
            .join("gtp-no-such-folder-xyz")
            .join("a.tif");
        let err = encode_tiff16(&picture(4, 4), OutputSpace::Srgb, &path, None).unwrap_err();
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
        write_output_tiff16(&dir.0.join("big.tif"), &out, None).unwrap();
        eprintln!("TIFF write: {:?}", start.elapsed());
        let start = std::time::Instant::now();
        encode_jpeg(&img, OutputSpace::Srgb, 90, &dir.0.join("big.jpg"), None).unwrap();
        eprintln!("JPEG (transform + encode): {:?}", start.elapsed());
    }
}
