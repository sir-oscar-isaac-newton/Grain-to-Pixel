//! Export encoders: a converted image ([`to_output`]) written as a 16-bit TIFF or an 8-bit JPEG,
//! each with its ICC profile embedded. Both write through a temporary file and a rename.

use std::borrow::Cow;
use std::io::{Seek, Write};
use std::path::Path;

use gtp_core::recipe::OutputSpace;
use gtp_core::{AppError, ErrorCode, ImageF32};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use tiff::encoder::colortype::{ColorType as TiffColorType, Gray16, RGB16};
use tiff::encoder::compression::DeflateLevel;
use tiff::encoder::{Compression as TiffCompression, TiffEncoder, TiffValue};
use tiff::tags::{Predictor, Tag, Type};

use crate::icc::{to_grey_output, to_output, Grey16Output, Tiff16Output};
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

/// Writes one 16-bit TIFF of colour type `C` (`RGB16` or `Gray16`) with its profile and tags.
fn encode_tiff_as<C: TiffColorType<Inner = u16>>(
    writer: impl Write + Seek,
    (width, height): (u32, u32),
    data: &[u16],
    icc: &[u8],
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
    let mut tiff = encoder.new_image::<C>(width, height)?;
    let dir = tiff.encoder();
    dir.write_tag(Tag::IccProfile, UndefinedValue(icc))?;
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
    tiff.write_data(data)
}

fn encode_tiff(
    writer: impl Write + Seek,
    image: &Tiff16Output,
    metadata: Option<&ExportMetadata>,
) -> tiff::TiffResult<()> {
    encode_tiff_as::<RGB16>(
        writer,
        (image.width, image.height),
        &image.data,
        &image.icc,
        metadata,
    )
}

fn encode_tiff_grey(
    writer: impl Write + Seek,
    image: &Grey16Output,
    metadata: Option<&ExportMetadata>,
) -> tiff::TiffResult<()> {
    encode_tiff_as::<Gray16>(
        writer,
        (image.width, image.height),
        &image.data,
        &image.icc,
        metadata,
    )
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

/// Writes an already converted grey image as a Deflate-compressed one-channel 16-bit TIFF
/// (BlackIsZero) with its grey profile, and the metadata when given.
pub fn write_grey_output_tiff16(
    path: &Path,
    image: &Grey16Output,
    metadata: Option<&ExportMetadata>,
) -> Result<(), AppError> {
    write_through_temp(path, |writer| {
        encode_tiff_grey(writer, image, metadata)
            .map_err(|err| encode_failed("The TIFF file could not be written.", err))
    })
}

/// Converts a linear Rec.2020 image to one grey channel and writes it as a 16-bit TIFF, with
/// `metadata` when given.
pub fn encode_tiff16_grey(
    image: &ImageF32,
    path: &Path,
    metadata: Option<&ExportMetadata>,
) -> Result<(), AppError> {
    write_grey_output_tiff16(path, &to_grey_output(image)?, metadata)
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

/// Checks the quality and the size of a JPEG and returns the width and height.
fn check_jpeg(image: &ImageF32, quality: u8) -> Result<(u16, u16), AppError> {
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
    Ok((width, height))
}

/// 16 to 8 bits on the same curve: round(v / 257).
fn to_8_bit(data: &[u16]) -> Vec<u8> {
    data.iter()
        .map(|&v| ((u32::from(v) + 128) / 257) as u8)
        .collect()
}

/// Encodes 8-bit pixels of `color` as a JPEG with the profile and the Exif block.
fn jpeg_from_8_bit(
    pixels: &[u8],
    (width, height): (u16, u16),
    color: ColorType,
    icc: &[u8],
    quality: u8,
    metadata: Option<&ExportMetadata>,
) -> Result<Vec<u8>, AppError> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, quality);
    encoder.set_sampling_factor(SamplingFactor::F_1_1);
    encoder
        .add_icc_profile(icc)
        .map_err(|err| encode_failed("The JPEG file could not be written.", err))?;
    if let Some(block) = metadata.map(jpeg_exif_block).transpose()?.flatten() {
        encoder
            .add_exif_metadata(&block)
            .map_err(|err| encode_failed("The JPEG file could not be written.", err))?;
    }
    encoder
        .encode(pixels, width, height, color)
        .map_err(|err| encode_failed("The JPEG file could not be written.", err))?;
    Ok(bytes)
}

/// [`encode_jpeg`] to memory.
pub fn encode_jpeg_bytes(
    image: &ImageF32,
    space: OutputSpace,
    quality: u8,
    metadata: Option<&ExportMetadata>,
) -> Result<Vec<u8>, AppError> {
    let size = check_jpeg(image, quality)?;
    let output = to_output(image, space)?;
    jpeg_from_8_bit(
        &to_8_bit(&output.data),
        size,
        ColorType::Rgb,
        &output.icc,
        quality,
        metadata,
    )
}

/// Converts a linear Rec.2020 image to one grey channel and writes it as an 8-bit greyscale
/// baseline JPEG of the given quality (1 to 100) with the grey profile, with `metadata` when
/// given.
pub fn encode_jpeg_grey(
    image: &ImageF32,
    quality: u8,
    path: &Path,
    metadata: Option<&ExportMetadata>,
) -> Result<(), AppError> {
    let bytes = encode_jpeg_grey_bytes(image, quality, metadata)?;
    write_through_temp(path, |writer| {
        writer
            .write_all(&bytes)
            .map_err(|err| AppError::from_io(&err))
    })
}

/// [`encode_jpeg_grey`] to memory.
pub fn encode_jpeg_grey_bytes(
    image: &ImageF32,
    quality: u8,
    metadata: Option<&ExportMetadata>,
) -> Result<Vec<u8>, AppError> {
    let size = check_jpeg(image, quality)?;
    let output = to_grey_output(image)?;
    jpeg_from_8_bit(
        &to_8_bit(&output.data),
        size,
        ColorType::Luma,
        &output.icc,
        quality,
        metadata,
    )
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use gtp_core::testing::TempDir;
    use lcms2::{ColorSpaceSignature, Profile};
    use tiff::decoder::Decoder;

    use super::*;
    use crate::{grey_icc_profile, read_tiff16, ExifDate, Limits};

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
            let path = dir.join(format!("{space:?}.tif"));
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
        let path = dir.join("frame.jpg");
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
            description: Some("Kodak Portra 400 (at ISO 800), Roll 12".into()),
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
            "Kodak Portra 400 (at ISO 800), Roll 12"
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

    /// The JPEG without its ICC profile segments (APP2 `ICC_PROFILE\0`).
    ///
    /// The profile header carries its creation time to the second, so two
    /// exports made at different moments differ there and nowhere else.
    /// Segments are walked up to the start of the scan; the rest of the file
    /// (the compressed picture and the end marker) is kept as one block.
    fn jpeg_without_icc(bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes[..2].to_vec();
        let mut i = 2;
        while bytes[i + 1] != 0xDA {
            let len = 2 + usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
            let segment = &bytes[i..i + len];
            let is_icc = segment[1] == 0xE2 && segment[4..].starts_with(b"ICC_PROFILE\0");
            if !is_icc {
                out.extend_from_slice(segment);
            }
            i += len;
        }
        out.extend_from_slice(&bytes[i..]);
        out
    }

    /// The position of the low seconds byte in the creation time of the first
    /// ICC profile segment (profile header offset 35, after the 4-byte
    /// marker and length, the 12-byte tag and the 2-byte chunk numbers).
    fn icc_seconds_offset(bytes: &[u8]) -> usize {
        let mut i = 2;
        while bytes[i + 1] != 0xDA {
            let len = 2 + usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
            if bytes[i + 1] == 0xE2 && bytes[i + 4..].starts_with(b"ICC_PROFILE\0") {
                return i + 4 + 14 + 35;
            }
            i += len;
        }
        panic!("no ICC profile segment");
    }

    #[test]
    fn the_icc_creation_time_does_not_count_in_a_jpeg_comparison() {
        let dir = TempDir::new("icc-time");
        let jpg = dir.join("t.jpg");
        encode_jpeg(&picture(32, 20), OutputSpace::Srgb, 90, &jpg, None).unwrap();
        let bytes = std::fs::read(&jpg).unwrap();
        let seconds = icc_seconds_offset(&bytes);

        // Only the profile's creation time differs: counted as equal.
        let mut later = bytes.clone();
        later[seconds] = later[seconds].wrapping_add(1);
        assert_ne!(later, bytes);
        assert_eq!(jpeg_without_icc(&later), jpeg_without_icc(&bytes));

        // A byte of the picture data differs: counted as different.
        let mut other_picture = bytes.clone();
        let last_scan_byte = other_picture.len() - 3;
        other_picture[last_scan_byte] ^= 0x01;
        assert_ne!(jpeg_without_icc(&other_picture), jpeg_without_icc(&bytes));

        // A byte of another segment (the quantisation table) differs too.
        let mut other_segment = bytes.clone();
        let dqt = other_segment
            .windows(2)
            .position(|w| w == [0xFF, 0xDB])
            .unwrap();
        other_segment[dqt + 5] ^= 0x01;
        assert_ne!(jpeg_without_icc(&other_segment), jpeg_without_icc(&bytes));
    }

    #[test]
    fn a_jpeg_export_carries_its_metadata() {
        let dir = TempDir::new("jpeg-meta");
        let path = dir.join("m.jpg");
        let meta = sample_metadata();
        encode_jpeg(&picture(64, 40), OutputSpace::Srgb, 90, &path, Some(&meta)).unwrap();
        assert_sample_tags(&read_exif(&path));
    }

    #[test]
    fn a_tiff_export_carries_its_metadata() {
        let dir = TempDir::new("tiff-meta");
        let path = dir.join("m.tif");
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
        let jpg = dir.join("n.jpg");
        encode_jpeg(&img, OutputSpace::Srgb, 90, &jpg, None).unwrap();
        let bytes = std::fs::read(&jpg).unwrap();
        assert!(
            !bytes.windows(6).any(|w| w == b"Exif\0\0"),
            "no Exif block at all"
        );
        let tif = dir.join("n.tif");
        encode_tiff16(&img, OutputSpace::Srgb, &tif, None).unwrap();
        let exif = read_exif(&tif);
        for tag in ALL_TAGS {
            assert!(exif.get_field(tag, exif::In::PRIMARY).is_none(), "{tag}");
        }
        // Metadata with nothing in it writes nothing either.
        let empty = ExportMetadata::default();
        let jpg2 = dir.join("e.jpg");
        encode_jpeg(&img, OutputSpace::Srgb, 90, &jpg2, Some(&empty)).unwrap();
        let bytes2 = std::fs::read(&jpg2).unwrap();
        assert!(
            !bytes2.windows(6).any(|w| w == b"Exif\0\0"),
            "no Exif block at all"
        );
        // The profile's creation time (to the second) may differ between two
        // writes; everything else must be the same.
        assert_eq!(jpeg_without_icc(&bytes2), jpeg_without_icc(&bytes));
        let tif2 = dir.join("e.tif");
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
        let jpg = dir.join("u.jpg");
        let tif = dir.join("u.tif");
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
        let tif = dir.join("i.tif");
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

        let (plain, tagged) = (dir.join("p.tif"), dir.join("t.tif"));
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
                let path = dir.join(format!("q{q}.jpg"));
                encode_jpeg(&img, OutputSpace::Srgb, q, &path, None).unwrap();
                std::fs::metadata(&path).unwrap().len()
            })
            .collect();
        assert!(sizes.windows(2).all(|w| w[0] < w[1]), "{sizes:?}");
    }

    #[test]
    fn bad_quality_size_and_channels_are_rejected() {
        let dir = TempDir::new("errors");
        let path = dir.join("x.jpg");
        let img = picture(8, 8);
        for q in [0, 101] {
            let err = encode_jpeg(&img, OutputSpace::Srgb, q, &path, None).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
        }
        let wide = ImageF32::zeros(70_000, 1, 3).unwrap();
        let err = encode_jpeg(&wide, OutputSpace::Srgb, 90, &path, None).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let gray = ImageF32::zeros(4, 4, 1).unwrap();
        let err = encode_tiff16(&gray, OutputSpace::Srgb, &dir.join("g.tif"), None).unwrap_err();
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

    /// A smooth colourful picture with equal channels: a ramp from 0 to 1 plus a few fixed values.
    fn grey_ramp() -> ImageF32 {
        let mut data = Vec::new();
        for x in 0..256 {
            let v = x as f32 / 255.0;
            data.extend_from_slice(&[v, v, v]);
        }
        for v in [0.001_f32, 0.003, 0.18, 0.5, 0.999] {
            data.extend_from_slice(&[v, v, v]);
        }
        let width = (data.len() / 3) as u32;
        ImageF32::new(width, 1, 3, data).unwrap()
    }

    /// A pseudo-random mildly colourful image inside the sRGB gamut: mid tones with each channel
    /// within 15% of a random grey (a fixed LCG, so the test is repeatable).
    fn colourful() -> ImageF32 {
        let mut state = 12345u32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32
        };
        let mut data = Vec::new();
        for _ in 0..64 * 16 {
            let base = 0.2 + 0.6 * next();
            data.extend((0..3).map(|_| base * (1.0 + 0.15 * (next() - 0.5))));
        }
        ImageF32::new(64, 16, 3, data).unwrap()
    }

    /// The luma of the sRGB export of a pixel: the linear sRGB pixel's Y re-encoded with the
    /// sRGB curve, 16-bit. The pixel goes through Little CMS to sRGB, is made linear again, and
    /// weighted with the Rec.709 coefficients.
    fn srgb_luma16(rgb16: &[u16]) -> f64 {
        let lin = |v: u16| {
            let c = f64::from(v) / 65535.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let y = 0.2126 * lin(rgb16[0]) + 0.7152 * lin(rgb16[1]) + 0.0722 * lin(rgb16[2]);
        let e = if y <= 0.003_130_8 {
            y * 12.92
        } else {
            1.055 * y.powf(1.0 / 2.4) - 0.055
        };
        e * 65535.0
    }

    /// The grey TIFF values against the luma of the sRGB RGB export of the same image.
    ///
    /// Tolerance: 65 of 65535 (0.1% of full scale) per pixel, both on the equal-channel ramp,
    /// where the grey value is simply the R value of the sRGB export, and on a mildly colourful
    /// image kept inside the sRGB gamut (outside it the sRGB export clips per channel before
    /// the luma is taken, and the two legitimately differ).
    #[test]
    fn a_grey_tiff_matches_the_luma_of_the_srgb_export() {
        let dir = TempDir::new("grey-luma");
        let ramp = grey_ramp();
        let path = dir.join("ramp.tif");
        encode_tiff16_grey(&ramp, &path, None).unwrap();
        let back = read_tiff16(&path, &Limits::default()).unwrap();
        let rgb = to_output(&ramp, OutputSpace::Srgb).unwrap();
        assert_eq!((back.channels, back.data.len()), (1, rgb.data.len() / 3));
        for (i, &g) in back.data.iter().enumerate() {
            let r = rgb.data[3 * i];
            assert!(g.abs_diff(r) <= 65, "pixel {i}: grey {g} vs sRGB {r}");
        }

        let image = colourful();
        let grey = to_grey_output(&image).unwrap();
        let rgb = to_output(&image, OutputSpace::Srgb).unwrap();
        for (i, &g) in grey.data.iter().enumerate() {
            let luma = srgb_luma16(&rgb.data[3 * i..3 * i + 3]);
            assert!(
                (f64::from(g) - luma).abs() <= 65.0,
                "pixel {i}: grey {g} vs sRGB luma {luma}"
            );
        }
    }

    /// The 8-bit grey JPEG input against the sRGB export, to 2 of 255 levels on a smooth ramp.
    /// (No JPEG decoder is in the tree, so the picture the encoder is given is compared.)
    #[test]
    fn the_grey_jpeg_encoder_input_matches_the_srgb_export() {
        let ramp = grey_ramp();
        let grey = to_8_bit(&to_grey_output(&ramp).unwrap().data);
        let rgb = to_8_bit(&to_output(&ramp, OutputSpace::Srgb).unwrap().data);
        for (i, &g) in grey.iter().enumerate() {
            assert!(g.abs_diff(rgb[3 * i]) <= 2, "pixel {i}");
        }
    }

    #[test]
    fn a_grey_tiff_is_one_channel_16_bit_with_the_grey_profile() {
        let dir = TempDir::new("grey-tiff");
        let path = dir.join("g.tif");
        encode_tiff16_grey(&grey_ramp(), &path, Some(&sample_metadata())).unwrap();
        let mut decoder = Decoder::new(File::open(&path).unwrap()).unwrap();
        assert_eq!(
            decoder.colortype().unwrap(),
            tiff::ColorType::Gray(16),
            "one channel, 16 bits"
        );
        let icc = decoder.get_tag_u8_vec(Tag::IccProfile).unwrap();
        assert_eq!(
            without_date(&icc),
            without_date(&grey_icc_profile().unwrap())
        );
        let parsed = Profile::new_icc(&icc).unwrap();
        assert_eq!(parsed.color_space(), ColorSpaceSignature::GrayData);
        assert_sample_tags(&read_exif(&path));
    }

    #[test]
    fn a_grey_jpeg_is_one_component_with_the_grey_profile_and_exif() {
        let dir = TempDir::new("grey-jpeg");
        let path = dir.join("g.jpg");
        encode_jpeg_grey(&picture(64, 40), 90, &path, Some(&sample_metadata())).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // The start-of-frame segment (SOF0, 0xFFC0): length, precision, height, width, components.
        let sof = bytes.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
        assert_eq!(bytes[sof + 4], 8, "8 bits");
        assert_eq!(bytes[sof + 9], 1, "one component");
        assert_eq!(
            without_date(&jpeg_icc(&bytes)),
            without_date(&grey_icc_profile().unwrap())
        );
        assert_sample_tags(&read_exif(&path));
    }

    #[test]
    fn grey_encoders_reject_bad_input() {
        let dir = TempDir::new("grey-errors");
        let img = picture(8, 8);
        for q in [0, 101] {
            let err = encode_jpeg_grey(&img, q, &dir.join("x.jpg"), None).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
        }
        let wide = ImageF32::zeros(70_000, 1, 3).unwrap();
        let err = encode_jpeg_grey(&wide, 90, &dir.join("x.jpg"), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let one = ImageF32::zeros(4, 4, 1).unwrap();
        let err = encode_tiff16_grey(&one, &dir.join("g.tif"), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
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
        write_output_tiff16(&dir.join("big.tif"), &out, None).unwrap();
        eprintln!("TIFF write: {:?}", start.elapsed());
        let start = std::time::Instant::now();
        encode_jpeg(&img, OutputSpace::Srgb, 90, &dir.join("big.jpg"), None).unwrap();
        eprintln!("JPEG (transform + encode): {:?}", start.elapsed());
    }
}
