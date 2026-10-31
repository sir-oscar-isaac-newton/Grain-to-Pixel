//! Safe 16-bit TIFF reading and writing for scans, infrared planes and library originals.
//!
//! Accepted layouts: 16-bit gray (1 channel), RGB (3) and RGB plus a fourth channel (4, "RGBI":
//! red, green, blue, infrared). A fourth channel is accepted when the file has no ExtraSamples tag
//! or marks it as alpha; one declared "unspecified" is refused, because the TIFF decoder would drop
//! it silently. Anything else is `UnsupportedFormat`. Files are untrusted (docs/specification.md section 9): the declared
//! size is checked against [`Limits`] before any pixel buffer is allocated, and every failure is
//! a typed [`AppError`], never a panic.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use gtp_core::{AppError, ErrorCode, ImageF32};
use tiff::decoder::{Decoder, DecodingResult, Limits as DecoderLimits};
use tiff::encoder::colortype::{Gray16, RGB16, RGBA16};
use tiff::encoder::compression::DeflateLevel;
use tiff::encoder::{Compression as TiffCompression, TiffEncoder};
use tiff::tags::{Predictor, Tag};
use tiff::{ColorType, TiffError};

use crate::limits::Limits;

const BYTES_PER_SAMPLE: u8 = 2;
/// Each IFD value (tag payload) may be at most this large; real files need a few hundred bytes.
const MAX_IFD_VALUE_BYTES: usize = 1024 * 1024;
const MAX_INTERMEDIATE_BYTES: u64 = 128 * 1024 * 1024;

/// 16-bit samples, row-major, interleaved. `channels` is 1, 3 or 4; by convention the fourth\n/// channel of a 4-channel image is infrared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tiff16 {
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub data: Vec<u16>,
}

/// How a file is compressed when written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    None,
    /// Deflate with the horizontal predictor: lossless and much smaller for smooth scans.
    #[default]
    DeflatePredictor,
}

fn unsupported(message: &str, details: impl fmt::Display) -> AppError {
    AppError::new(ErrorCode::UnsupportedFormat, message).with_details(details.to_string())
}

fn map_tiff(err: TiffError) -> AppError {
    match err {
        TiffError::IoError(io) if io.kind() == io::ErrorKind::UnexpectedEof => {
            unsupported("This TIFF file is incomplete.", io)
        }
        TiffError::IoError(io) => AppError::from_io(&io),
        TiffError::LimitsExceeded | TiffError::IntSizeError => AppError::new(
            ErrorCode::InvalidInput,
            "This image is larger than the app accepts.",
        )
        .with_details(err.to_string()),
        other => unsupported("This is not a TIFF file the app can read.", other),
    }
}

/// Number of 16-bit channels of a decoded colour type, or `UnsupportedFormat`.
fn channels_of(color: ColorType) -> Result<u8, AppError> {
    match color {
        ColorType::Gray(16) => Ok(1),
        ColorType::RGB(16) => Ok(3),
        ColorType::RGBA(16)
        | ColorType::Multiband {
            bit_depth: 16,
            num_samples: 4,
        } => Ok(4),
        other => Err(unsupported(
            "Only 16-bit gray, RGB and RGB plus infrared TIFF files are supported.",
            format!("{other:?}"),
        )),
    }
}

/// The decoder's own caps: pixel buffers (decoding, intermediate) and IFD tag payloads.
fn decoder_limits(decoding: usize, intermediate: usize) -> DecoderLimits {
    let mut limits = DecoderLimits::unlimited();
    limits.decoding_buffer_size = decoding;
    limits.intermediate_buffer_size = intermediate;
    limits.ifd_value_size = MAX_IFD_VALUE_BYTES;
    limits
}

fn open(path: &Path) -> Result<File, AppError> {
    File::open(path).map_err(|err| AppError::from_io(&err))
}

/// Reads a 16-bit TIFF. The size limits are applied before the pixels are decoded.
pub fn read_tiff16(path: &Path, limits: &Limits) -> Result<Tiff16, AppError> {
    let reader = BufReader::new(open(path)?);
    let buffer_cap = usize::try_from(limits.max_decoded_bytes).unwrap_or(usize::MAX);
    let intermediate_cap = usize::try_from(MAX_INTERMEDIATE_BYTES)
        .unwrap_or(usize::MAX)
        .min(buffer_cap);
    let mut decoder = Decoder::new(reader)
        .map_err(map_tiff)?
        .with_limits(decoder_limits(buffer_cap, intermediate_cap));

    let (width, height) = decoder.dimensions().map_err(map_tiff)?;
    let channels = channels_of(decoder.colortype().map_err(map_tiff)?)?;
    let samples = limits.check(width, height, channels, BYTES_PER_SAMPLE)?;

    // The decoder returns only the colour samples plus an alpha-marked one. A file with another
    // kind of extra sample (for example "unspecified") would lose that channel silently, so it is
    // refused instead.
    let stored = decoder
        .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)
        .map_err(map_tiff)?;
    if stored.is_some_and(|stored| stored != u16::from(channels)) {
        return Err(unsupported(
            "This TIFF has an extra channel the reader cannot expose yet.",
            format!(
                "{stored:?} samples per pixel but {channels} readable; run `cargo run -p gtp-io --example tiff_info <file>`"
            ),
        ));
    }
    let DecodingResult::U16(data) = decoder.read_image().map_err(map_tiff)? else {
        return Err(unsupported(
            "Only 16-bit TIFF files are supported.",
            "the decoder did not return 16-bit samples",
        ));
    };
    if data.len() != samples {
        return Err(unsupported(
            "This TIFF file does not match its header.",
            format!("expected {samples} samples, found {}", data.len()),
        ));
    }
    Ok(Tiff16 {
        width,
        height,
        channels,
        data,
    })
}

fn temp_path_for(path: &Path) -> Result<PathBuf, AppError> {
    let name = path.file_name().ok_or_else(|| {
        AppError::new(ErrorCode::InvalidInput, "That is not a file path.")
            .with_details(path.display().to_string())
    })?;
    let mut temp_name = name.to_os_string();
    temp_name.push(".gtp-tmp");
    Ok(path.with_file_name(temp_name))
}

fn encode(
    writer: &mut BufWriter<File>,
    image: &Tiff16,
    compression: Compression,
) -> Result<(), TiffError> {
    let mut encoder = TiffEncoder::new(&mut *writer)?;
    if compression == Compression::DeflatePredictor {
        encoder = encoder
            .with_compression(TiffCompression::Deflate(DeflateLevel::Balanced))
            .with_predictor(Predictor::Horizontal);
    }
    let (w, h) = (image.width, image.height);
    match image.channels {
        1 => encoder.write_image::<Gray16>(w, h, &image.data),
        3 => encoder.write_image::<RGB16>(w, h, &image.data),
        // Four interleaved samples per pixel and no ExtraSamples tag, which the reader returns whole.
        // (Viewers may show the fourth channel as alpha; the library stores IR as a separate file.)
        // The predictor needs the sample count built into the colour type, hence RGBA16.
        _ => encoder.write_image::<RGBA16>(w, h, &image.data),
    }
}

/// Writes a 16-bit TIFF through a temporary file and a rename, so a failure never leaves a
/// half-written file at `path`.
pub fn write_tiff16(path: &Path, image: &Tiff16, compression: Compression) -> Result<(), AppError> {
    if !matches!(image.channels, 1 | 3 | 4) {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Only 1, 3 or 4 channels can be written.",
        )
        .with_details(format!("{} channels", image.channels)));
    }
    let expected = Limits {
        max_side: u32::MAX,
        max_decoded_bytes: u64::MAX,
    }
    .check(image.width, image.height, image.channels, BYTES_PER_SAMPLE)?;
    if image.data.len() != expected {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The image data does not match its size.",
        )
        .with_details(format!(
            "expected {expected} samples, got {}",
            image.data.len()
        )));
    }

    write_through_temp(path, |writer| {
        encode(writer, image, compression).map_err(map_tiff)
    })
}

/// Writes a file through a temporary file next to `path` and a rename, so a failure never leaves a
/// half-written file at `path`. `write` fills the buffered temporary file.
pub(crate) fn write_through_temp(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<File>) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let temp = temp_path_for(path)?;
    let result = (|| -> Result<(), AppError> {
        let file = File::create(&temp).map_err(|err| AppError::from_io(&err))?;
        let mut writer = BufWriter::new(file);
        write(&mut writer)?;
        writer.flush().map_err(|err| AppError::from_io(&err))?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|err| AppError::from_io(&err))?;
        Ok(())
    })();
    match result.and_then(|()| fs::rename(&temp, path).map_err(|err| AppError::from_io(&err))) {
        Ok(()) => Ok(()),
        Err(err) => {
            // Best effort: the original error is the one worth reporting.
            let _ = fs::remove_file(&temp);
            Err(err)
        }
    }
}

/// Scales 16-bit samples to linear `[0, 1]` floats. Scale only: no gamma, no profile.
pub fn to_f32_linear(image: &Tiff16) -> Result<ImageF32, AppError> {
    let data = image.data.iter().map(|&v| f32::from(v) / 65535.0).collect();
    ImageF32::new(image.width, image.height, image.channels, data)
}

/// Rounds linear floats to 16-bit samples; values are clamped to `[0, 1]` and NaN becomes 0.
pub fn from_f32_linear(image: &ImageF32) -> Tiff16 {
    let data = image
        .data
        .iter()
        .map(|&v| {
            if v.is_nan() {
                0
            } else {
                (v.clamp(0.0, 1.0) * 65535.0).round() as u16
            }
        })
        .collect();
    Tiff16 {
        width: image.width,
        height: image.height,
        channels: image.channels,
        data,
    }
}

/// Splits an RGBI image into its RGB image and its infrared plane (1 channel).
pub fn split_rgbi(image: &Tiff16) -> Result<(Tiff16, Tiff16), AppError> {
    if image.channels != 4 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Only a 4-channel image can be split into RGB and infrared.",
        )
        .with_details(format!("{} channels", image.channels)));
    }
    let pixels = image.data.len() / 4;
    let mut rgb = Vec::with_capacity(pixels * 3);
    let mut ir = Vec::with_capacity(pixels);
    let (pixels4, _) = image.data.as_chunks::<4>();
    for [r, g, b, infrared] in pixels4 {
        rgb.extend_from_slice(&[*r, *g, *b]);
        ir.push(*infrared);
    }
    let plane = |channels, data| Tiff16 {
        width: image.width,
        height: image.height,
        channels,
        data,
    };
    Ok((plane(3, rgb), plane(1, ir)))
}

/// Joins an RGB image and an infrared plane of the same size into one RGBI image.
pub fn merge_rgbi(rgb: &Tiff16, ir: &Tiff16) -> Result<Tiff16, AppError> {
    if rgb.channels != 3 || ir.channels != 1 || rgb.width != ir.width || rgb.height != ir.height {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The RGB image and the infrared plane must be the same size.",
        )
        .with_details(format!(
            "rgb {}x{}x{}, ir {}x{}x{}",
            rgb.width, rgb.height, rgb.channels, ir.width, ir.height, ir.channels
        )));
    }
    let mut data = Vec::with_capacity(rgb.data.len() + ir.data.len());
    let (pixels3, _) = rgb.data.as_chunks::<3>();
    for (pixel, &infrared) in pixels3.iter().zip(&ir.data) {
        data.extend_from_slice(pixel);
        data.push(infrared);
    }
    Ok(Tiff16 {
        width: rgb.width,
        height: rgb.height,
        channels: 4,
        data,
    })
}

/// The structure of a TIFF file as stored, without its pixels (for diagnosing files from other
/// software). Values are the raw TIFF tag values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiffLayout {
    pub width: u32,
    pub height: u32,
    pub bits_per_sample: Vec<u16>,
    pub samples_per_pixel: Option<u16>,
    /// 1 black-is-zero, 2 RGB, 5 CMYK, ...
    pub photometric: Option<u16>,
    /// 1 none, 5 LZW, 8 or 32946 Deflate, 32773 PackBits.
    pub compression: Option<u16>,
    /// 1 none, 2 horizontal differencing.
    pub predictor: Option<u16>,
    /// 0 unspecified (for example infrared), 1 associated alpha, 2 unassociated alpha.
    pub extra_samples: Vec<u16>,
    /// 1 chunky (interleaved), 2 planar.
    pub planar_configuration: Option<u16>,
}

impl fmt::Display for TiffLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn show(value: Option<u16>) -> String {
            value.map_or_else(|| "absent".to_owned(), |v| v.to_string())
        }
        writeln!(f, "size:                 {} x {}", self.width, self.height)?;
        writeln!(f, "bits per sample:      {:?}", self.bits_per_sample)?;
        writeln!(f, "samples per pixel:    {}", show(self.samples_per_pixel))?;
        writeln!(f, "photometric:          {}", show(self.photometric))?;
        writeln!(f, "compression:          {}", show(self.compression))?;
        writeln!(f, "predictor:            {}", show(self.predictor))?;
        writeln!(f, "extra samples:        {:?}", self.extra_samples)?;
        write!(
            f,
            "planar configuration: {}",
            show(self.planar_configuration)
        )
    }
}

/// Reads the layout tags of a TIFF file. No pixel data is decoded or allocated.
pub fn inspect_tiff(path: &Path) -> Result<TiffLayout, AppError> {
    let mut decoder = Decoder::new(BufReader::new(open(path)?))
        .map_err(map_tiff)?
        .with_limits(decoder_limits(MAX_IFD_VALUE_BYTES, MAX_IFD_VALUE_BYTES));
    let (width, height) = decoder.dimensions().map_err(map_tiff)?;
    let mut single = |tag: Tag| -> Result<Option<u16>, AppError> {
        decoder.find_tag_unsigned::<u16>(tag).map_err(map_tiff)
    };
    let samples_per_pixel = single(Tag::SamplesPerPixel)?;
    let photometric = single(Tag::PhotometricInterpretation)?;
    let compression = single(Tag::Compression)?;
    let predictor = single(Tag::Predictor)?;
    let planar_configuration = single(Tag::PlanarConfiguration)?;
    let mut list = |tag: Tag| -> Result<Vec<u16>, AppError> {
        Ok(decoder
            .find_tag_unsigned_vec::<u16>(tag)
            .map_err(map_tiff)?
            .unwrap_or_default())
    };
    Ok(TiffLayout {
        width,
        height,
        bits_per_sample: list(Tag::BitsPerSample)?,
        samples_per_pixel,
        photometric,
        compression,
        predictor,
        extra_samples: list(Tag::ExtraSamples)?,
        planar_configuration,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use gtp_core::testing::synthetic::{self, Params};
    use proptest::prelude::*;
    use tiff::encoder::colortype::{Gray8, CMYK16, RGB8, RGBA8};
    use tiff::tags::ExtraSamples;

    use super::*;

    type EncodeFn<'a> = dyn Fn(&mut TiffEncoder<BufWriter<File>>) -> Result<(), TiffError> + 'a;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_path(label: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("gtp-io-{label}-{}-{n}.tif", std::process::id()))
    }

    fn synthetic_rgb() -> Tiff16 {
        let scan = synthetic::negative(1, &Params::default()).unwrap();
        from_f32_linear(&scan.image)
    }

    fn synthetic_rgbi() -> Tiff16 {
        let scan = synthetic::negative(
            2,
            &Params {
                dust_specks: 4,
                with_ir: true,
                ..Params::default()
            },
        )
        .unwrap();
        let ir = from_f32_linear(scan.ir.as_ref().unwrap());
        merge_rgbi(&from_f32_linear(&scan.image), &ir).unwrap()
    }

    fn gray() -> Tiff16 {
        split_rgbi(&synthetic_rgbi()).unwrap().1
    }

    fn round_trip(image: &Tiff16, compression: Compression) -> Tiff16 {
        let path = temp_path("rt");
        write_tiff16(&path, image, compression).unwrap();
        let back = read_tiff16(&path, &Limits::default()).unwrap();
        fs::remove_file(&path).unwrap();
        back
    }

    #[test]
    fn gray_rgb_and_rgbi_round_trip_bit_exactly_with_both_compressions() {
        for image in [gray(), synthetic_rgb(), synthetic_rgbi()] {
            for compression in [Compression::None, Compression::DeflatePredictor] {
                let back = round_trip(&image, compression);
                assert_eq!(back, image, "{} channels, {compression:?}", image.channels);
            }
        }
    }

    #[test]
    fn every_16_bit_value_survives() {
        let data: Vec<u16> = (0..=u16::MAX).collect();
        let image = Tiff16 {
            width: 256,
            height: 256,
            channels: 1,
            data,
        };
        assert_eq!(round_trip(&image, Compression::DeflatePredictor), image);
    }

    #[test]
    fn deflate_with_predictor_is_smaller_than_raw_on_a_synthetic_negative() {
        let scan = synthetic::negative(
            3,
            &Params {
                width: 256,
                height: 192,
                ..Params::default()
            },
        )
        .unwrap();
        let image = from_f32_linear(&scan.image);
        let (raw, packed) = (temp_path("raw"), temp_path("packed"));
        write_tiff16(&raw, &image, Compression::None).unwrap();
        write_tiff16(&packed, &image, Compression::DeflatePredictor).unwrap();
        let (raw_len, packed_len) = (
            fs::metadata(&raw).unwrap().len(),
            fs::metadata(&packed).unwrap().len(),
        );
        assert!(packed_len < raw_len, "{packed_len} !< {raw_len}");
        fs::remove_file(raw).unwrap();
        fs::remove_file(packed).unwrap();
    }

    #[test]
    fn an_rgbi_file_is_written_with_four_samples_and_read_back_whole() {
        let image = synthetic_rgbi();
        let path = temp_path("extra");
        write_tiff16(&path, &image, Compression::DeflatePredictor).unwrap();
        let layout = inspect_tiff(&path).unwrap();
        assert_eq!(layout.samples_per_pixel, Some(4));
        assert!(layout.extra_samples.is_empty());
        assert_eq!(read_tiff16(&path, &Limits::default()).unwrap(), image);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn an_extra_channel_the_decoder_would_drop_is_refused_not_lost() {
        // RGB plus a sample declared "unspecified": the TIFF decoder returns only 3 channels.
        let path = temp_path("unspecified");
        let mut encoder = TiffEncoder::new(BufWriter::new(File::create(&path).unwrap())).unwrap();
        let mut directory = encoder.new_image::<RGB16>(4, 2).unwrap();
        directory
            .extra_samples(&[ExtraSamples::Unspecified])
            .unwrap();
        directory.write_data(&[7u16; 32]).unwrap();
        drop(encoder);

        let err = read_tiff16(&path, &Limits::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::UnsupportedFormat);
        assert!(err.message.contains("extra channel"), "{err}");
        assert_eq!(
            inspect_tiff(&path).unwrap().extra_samples,
            [0],
            "the layout tool still shows it"
        );
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn inspect_reports_the_layout_without_decoding_pixels() {
        let path = temp_path("layout");
        let image = synthetic_rgbi();
        write_tiff16(&path, &image, Compression::DeflatePredictor).unwrap();
        let layout = inspect_tiff(&path).unwrap();
        assert_eq!((layout.width, layout.height), (image.width, image.height));
        assert_eq!(layout.bits_per_sample, [16, 16, 16, 16]);
        assert_eq!(layout.samples_per_pixel, Some(4));
        assert_eq!(layout.predictor, Some(2));
        let text = layout.to_string();
        assert!(text.contains("samples per pixel:    4"), "{text}");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rgbi_splits_into_rgb_and_ir_and_merges_back() {
        let rgbi = synthetic_rgbi();
        let (rgb, ir) = split_rgbi(&rgbi).unwrap();
        assert_eq!((rgb.channels, ir.channels), (3, 1));
        assert_eq!(rgb.data.len(), ir.data.len() * 3);
        assert_eq!(merge_rgbi(&rgb, &ir).unwrap(), rgbi);
        assert_eq!(split_rgbi(&rgb).unwrap_err().code, ErrorCode::InvalidInput);
        let smaller = Tiff16 {
            width: 1,
            height: 1,
            channels: 1,
            data: vec![0],
        };
        assert_eq!(
            merge_rgbi(&rgb, &smaller).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn float_conversion_scales_rounds_clamps_and_inverts() {
        let image = Tiff16 {
            width: 3,
            height: 1,
            channels: 1,
            data: vec![0, 32768, 65535],
        };
        let floats = to_f32_linear(&image).unwrap();
        assert_eq!(floats.data[0], 0.0);
        assert_eq!(floats.data[2], 1.0);
        assert_eq!(from_f32_linear(&floats), image);

        let odd = ImageF32::new(4, 1, 1, vec![-1.0, 2.0, f32::NAN, 0.5]).unwrap();
        assert_eq!(from_f32_linear(&odd).data, [0, 65535, 0, 32768]);
    }

    /// A little-endian TIFF with one strip: just enough structure to declare a size.
    fn crafted_gray16(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        let entries: [(u16, u16, u32, u32); 9] = [
            (256, 4, 1, width),  // ImageWidth
            (257, 4, 1, height), // ImageLength
            (258, 3, 1, 16),     // BitsPerSample
            (259, 3, 1, 1),      // Compression: none
            (262, 3, 1, 1),      // PhotometricInterpretation: black is zero
            (273, 4, 1, 200),    // StripOffsets
            (277, 3, 1, 1),      // SamplesPerPixel
            (278, 4, 1, height), // RowsPerStrip
            (279, 4, 1, 16),     // StripByteCounts
        ];
        bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, kind, count, value) in entries {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&kind.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.resize(216, 0);
        bytes
    }

    #[test]
    fn a_crafted_header_with_huge_dimensions_is_rejected_before_allocating() {
        for (w, h) in [(100_000, 100_000), (u32::MAX, u32::MAX), (65_535, 65_535)] {
            let path = temp_path("huge");
            fs::write(&path, crafted_gray16(w, h)).unwrap();
            let err = read_tiff16(&path, &Limits::default()).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{w}x{h}");
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn the_crafted_header_with_an_honest_size_is_a_valid_file() {
        // Guards the helper: the same header with an honest size is a valid file, so the
        // rejections above come from the limits and not from a malformed header.
        let path = temp_path("honest");
        fs::write(&path, crafted_gray16(2, 4)).unwrap();
        let image = read_tiff16(&path, &Limits::default()).unwrap();
        assert_eq!((image.width, image.height, image.channels), (2, 4, 1));
        assert_eq!(image.data, [0; 8]);
        let layout = inspect_tiff(&path).unwrap();
        assert_eq!((layout.width, layout.height), (2, 4));
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn a_file_over_the_byte_limit_is_rejected() {
        let path = temp_path("bytes");
        write_tiff16(&path, &synthetic_rgb(), Compression::None).unwrap();
        let tight = Limits {
            max_side: 20_000,
            max_decoded_bytes: 1000,
        };
        assert_eq!(
            read_tiff16(&path, &tight).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let short = Limits {
            max_side: 10,
            max_decoded_bytes: u64::MAX,
        };
        assert_eq!(
            read_tiff16(&path, &short).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn truncated_files_return_typed_errors() {
        let path = temp_path("trunc");
        write_tiff16(&path, &synthetic_rgb(), Compression::DeflatePredictor).unwrap();
        let full = fs::read(&path).unwrap();
        for cut in [0, 1, 4, 8, 20, full.len() / 2, full.len() - 1] {
            fs::write(&path, &full[..cut]).unwrap();
            let err = read_tiff16(&path, &Limits::default()).unwrap_err();
            assert!(
                matches!(err.code, ErrorCode::UnsupportedFormat | ErrorCode::Io),
                "cut at {cut}: {err}"
            );
        }
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn garbage_and_empty_files_are_not_tiffs() {
        let path = temp_path("garbage");
        for bytes in [&b""[..], b"not a tiff at all", &[0xff; 4096]] {
            fs::write(&path, bytes).unwrap();
            let err = read_tiff16(&path, &Limits::default()).unwrap_err();
            assert_eq!(err.code, ErrorCode::UnsupportedFormat);
        }
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_missing_file_is_not_found() {
        let err = read_tiff16(Path::new("nowhere/at/all.tif"), &Limits::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn other_layouts_are_unsupported() {
        let path = temp_path("layouts");
        let write = |f: &EncodeFn| {
            let mut encoder =
                TiffEncoder::new(BufWriter::new(File::create(&path).unwrap())).unwrap();
            f(&mut encoder).unwrap();
            drop(encoder);
        };
        write(&|e| e.write_image::<Gray8>(2, 2, &[0; 4]));
        write(&|e| e.write_image::<RGB8>(2, 2, &[0; 12]));
        assert_eq!(
            read_tiff16(&path, &Limits::default()).unwrap_err().code,
            ErrorCode::UnsupportedFormat
        );
        write(&|e| e.write_image::<RGBA8>(2, 2, &[0; 16]));
        assert_eq!(
            read_tiff16(&path, &Limits::default()).unwrap_err().code,
            ErrorCode::UnsupportedFormat
        );
        write(&|e| e.write_image::<CMYK16>(2, 2, &[0; 16]));
        assert_eq!(
            read_tiff16(&path, &Limits::default()).unwrap_err().code,
            ErrorCode::UnsupportedFormat
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn writing_checks_its_input_and_leaves_no_temporary_file() {
        let path = temp_path("write");
        let mut bad = synthetic_rgb();
        bad.data.pop();
        assert_eq!(
            write_tiff16(&path, &bad, Compression::None)
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
        let two = Tiff16 {
            width: 1,
            height: 1,
            channels: 2,
            data: vec![0, 0],
        };
        assert_eq!(
            write_tiff16(&path, &two, Compression::None)
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
        assert!(!path.exists());

        write_tiff16(&path, &synthetic_rgb(), Compression::None).unwrap();
        assert!(!temp_path_for(&path).unwrap().exists());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn writing_into_a_missing_directory_fails_with_a_typed_error() {
        let path = std::env::temp_dir()
            .join("gtp-io-no-such-dir")
            .join("x.tif");
        let err = write_tiff16(&path, &synthetic_rgb(), Compression::None).unwrap_err();
        assert!(matches!(err.code, ErrorCode::NotFound | ErrorCode::Io));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Flipping bytes of a valid file must give an image or a typed error, never a panic.
        #[test]
        fn damaged_files_never_panic(flips in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..12)) {
            let path = temp_path("fuzz");
            let image = Tiff16 { width: 16, height: 8, channels: 3, data: (0..384).map(|v| v * 100).collect() };
            write_tiff16(&path, &image, Compression::DeflatePredictor).unwrap();
            let mut bytes = fs::read(&path).unwrap();
            for (index, value) in flips {
                let at = index.index(bytes.len());
                bytes[at] = value;
            }
            fs::write(&path, &bytes).unwrap();
            let tight = Limits { max_side: 1000, max_decoded_bytes: 1 << 20 };
            let _ = read_tiff16(&path, &tight);
            let _ = inspect_tiff(&path);
            fs::remove_file(path).unwrap();
        }
    }
}
