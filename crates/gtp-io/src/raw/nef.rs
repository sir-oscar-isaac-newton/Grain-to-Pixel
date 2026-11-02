//! Reading camera files (NEF) into [`RawLinear`] with `rawler` (decision: ADR 0007).

use std::fs;
use std::path::Path;

use gtp_core::{AppError, ErrorCode};
use rawler::RawImageData;

use super::{Cfa, RawLinear};
use crate::Limits;

/// The largest camera file accepted (a D600 NEF is 24 to 26 MB). The file is untrusted input.
pub const MAX_RAW_FILE_BYTES: u64 = 200 * 1024 * 1024;

/// Reads a camera file into linear sensor samples. A trait so that merges can be tested with
/// constructed mosaics instead of camera files.
pub trait RawDecoder: Send + Sync {
    fn decode(&self, path: &Path, limits: &Limits) -> Result<RawLinear, AppError>;
}

/// The real decoder: `rawler`.
#[derive(Debug, Clone, Copy, Default)]
pub struct RawlerDecoder;

/// Reads the camera file at `path` with [`RawlerDecoder`].
pub fn decode_nef(path: &Path, limits: &Limits) -> Result<RawLinear, AppError> {
    RawlerDecoder.decode(path, limits)
}

fn unsupported(message: &str, details: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::UnsupportedFormat, message).with_details(details.to_string())
}

fn check_file_size(len: u64) -> Result<(), AppError> {
    if len > MAX_RAW_FILE_BYTES {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "This camera file is larger than the app accepts.",
        )
        .with_details(format!("{len} bytes (limit {MAX_RAW_FILE_BYTES})")));
    }
    Ok(())
}

impl RawDecoder for RawlerDecoder {
    fn decode(&self, path: &Path, limits: &Limits) -> Result<RawLinear, AppError> {
        let meta = fs::metadata(path).map_err(|err| AppError::from_io(&err))?;
        if !meta.is_file() {
            return Err(AppError::new(ErrorCode::NotFound, "That is not a file.")
                .with_details(path.display().to_string()));
        }
        check_file_size(meta.len())?;

        let raw = rawler::decode_file(path)
            .map_err(|err| unsupported("This is not a camera file the app can read.", err))?;
        let RawImageData::Integer(data) = raw.data else {
            return Err(unsupported(
                "This camera file has no integer sensor data.",
                path.display(),
            ));
        };
        if raw.cpp != 1 || !raw.camera.cfa.is_valid() || raw.camera.cfa.unique_colors() != 3 {
            return Err(unsupported(
                "Only Bayer sensors are supported.",
                format!("{} {}", raw.make, raw.model),
            ));
        }
        limits.check(raw.width as u32, raw.height as u32, 1, 2)?;
        if data.len() != raw.width * raw.height {
            return Err(unsupported(
                "The camera file's data does not match its size.",
                format!("{}x{} with {} samples", raw.width, raw.height, data.len()),
            ));
        }

        // The image area the camera gives; the whole frame if it does not say.
        let (x0, y0, width, height) = match raw.crop_area {
            Some(area)
                if area.p.x + area.d.w <= raw.width
                    && area.p.y + area.d.h <= raw.height
                    && area.d.w >= 2
                    && area.d.h >= 2 =>
            {
                (area.p.x, area.p.y, area.d.w, area.d.h)
            }
            _ => (0, 0, raw.width, raw.height),
        };
        let cfa = Cfa([
            raw.camera.cfa.color_at(y0, x0) as u8,
            raw.camera.cfa.color_at(y0, x0 + 1) as u8,
            raw.camera.cfa.color_at(y0 + 1, x0) as u8,
            raw.camera.cfa.color_at(y0 + 1, x0 + 1) as u8,
        ]);

        // The black level of each photosite of the cropped block (a repeat of 1 x 1 or 2 x 2).
        let levels = raw.blacklevel.as_vec();
        let black_at = |x: usize, y: usize| -> f32 {
            match (
                raw.blacklevel.width,
                raw.blacklevel.height,
                raw.blacklevel.cpp,
            ) {
                (2, 2, 1) if levels.len() == 4 => levels[((y & 1) << 1) | (x & 1)],
                _ => levels.iter().copied().fold(0.0, f32::max),
            }
        };
        let black = [
            black_at(x0, y0),
            black_at(x0 + 1, y0),
            black_at(x0, y0 + 1),
            black_at(x0 + 1, y0 + 1),
        ];
        let white = raw
            .whitelevel
            .0
            .iter()
            .copied()
            .max()
            .map_or(0.0, |w| w as f32);

        let mut cropped = Vec::with_capacity(width * height);
        for y in y0..y0 + height {
            cropped.extend_from_slice(&data[y * raw.width + x0..y * raw.width + x0 + width]);
        }
        let linear = RawLinear {
            width: width as u32,
            height: height as u32,
            cfa,
            black,
            white,
            data: cropped,
        };
        linear.validate()?;
        Ok(linear)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_above_the_size_limit_is_refused_before_it_is_read() {
        assert!(check_file_size(MAX_RAW_FILE_BYTES).is_ok());
        let err = check_file_size(MAX_RAW_FILE_BYTES + 1).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn a_missing_file_a_folder_and_a_file_that_is_not_a_camera_file_are_refused() {
        let dir = std::env::temp_dir().join(format!("gtp-nef-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let limits = Limits::default();
        assert_eq!(
            decode_nef(&dir.join("nothing.NEF"), &limits)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            decode_nef(&dir, &limits).unwrap_err().code,
            ErrorCode::NotFound
        );
        let text = dir.join("not-a-photo.NEF");
        std::fs::write(&text, b"this is not a camera file at all").unwrap();
        assert_eq!(
            decode_nef(&text, &limits).unwrap_err().code,
            ErrorCode::UnsupportedFormat
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A real D600 NEF from the private samples: 6016 x 4016 after the crop, RGGB, white 15520,
    /// black 0. Run by hand with `cargo test -p gtp-io --release -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the private D600 samples"]
    fn a_real_d600_file_decodes_to_the_cropped_mosaic() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../samples/private/Mamiya 645 Kodak Gold 200 120 Trichrome RGB/DSC_4652.NEF");
        if !path.is_file() {
            eprintln!("no private samples; skipped");
            return;
        }
        let started = std::time::Instant::now();
        let raw = decode_nef(&path, &Limits::default()).unwrap();
        eprintln!("decoded in {:?}", started.elapsed());
        assert_eq!((raw.width, raw.height), (6016, 4016));
        assert_eq!(raw.cfa, Cfa::RGGB);
        assert_eq!(raw.white, 15_520.0);
        assert_eq!(raw.black, [0.0; 4]);
        assert!(raw.data.iter().copied().max().unwrap() <= 15_520);
    }
}
