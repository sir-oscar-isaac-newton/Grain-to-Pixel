//! The capture devices of `GTP_FAKE_HW=1`: a [`FakeLight`], a [`FakeCamera`] that never runs out of
//! pictures, and the decoder that reads those pictures, so the capture commands and the capture
//! job run end to end without hardware.
//!
//! The camera's picture is a few bytes with a camera-like name: it has no sensor data. The
//! capture job stages each picture in a file named for its channel (`r-`, `g-`, `b-`), and
//! [`FakeRawDecoder`] answers with a deterministic synthetic mosaic for that channel: the lit
//! colour has a gradient well below the white level (no clipping), the others a dark leak, as in
//! a real exposure under one LED.

use std::path::Path;

// Re-exported so that the tests of the desktop shell can script a light without a dependency on
// `gtp-capture` (the shell depends on `gtp-app` only).
use gtp_capture::{CameraInfo, CapturedFile, FakeCamera};
pub use gtp_capture::{FakeLight, FakeLightHandle, LightInfo, LightSource, LightStatus};
use gtp_core::{AppError, ErrorCode};
use gtp_io::{Cfa, Limits, RawDecoder, RawLinear};

/// Width and height of the synthetic exposures.
const WIDTH: u32 = 192;
const HEIGHT: u32 = 128;
/// The sensor's white level in the synthetic mosaics.
const WHITE: f32 = 10_000.0;
/// What the colours the LED does not light read.
const LEAK: u16 = 120;
/// How bright each lit colour gets at the brightest corner, as a fraction of the white level.
const PEAK: [f32; 3] = [0.8, 0.6, 0.4];

/// The fake camera's identity in the camera list.
pub fn fake_camera_info() -> CameraInfo {
    CameraInfo {
        id: "fake".to_owned(),
        name: "Fake camera".to_owned(),
    }
}

/// A fake camera that delivers the same small picture for every shot.
pub fn fake_camera() -> FakeCamera {
    FakeCamera::new(Vec::new()).repeating(CapturedFile {
        name: "FAKE_0001.NEF".to_owned(),
        bytes: b"grain to pixel fake picture".to_vec(),
    })
}

/// Reads the staged pictures of the fake camera: the channel comes from the first letter of the
/// file name (`r`, `g` or `b`).
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeRawDecoder;

impl RawDecoder for FakeRawDecoder {
    fn decode(&self, path: &Path, _: &Limits) -> Result<RawLinear, AppError> {
        let lit = match path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.chars().next())
        {
            Some('r') => 0,
            Some('g') => 1,
            Some('b') => 2,
            _ => {
                return Err(AppError::new(
                    ErrorCode::UnsupportedFormat,
                    "This is not a picture of the fake camera.",
                ))
            }
        };
        let (w, h) = (WIDTH as usize, HEIGHT as usize);
        let mut data = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let value = if Cfa::RGGB.color_at(x, y) == lit {
                    // A smooth ramp from 30 % to 100 % of the peak, left to right and top to bottom.
                    let ramp = 0.3 + 0.7 * ((x + y) as f32 / (w + h - 2) as f32);
                    (WHITE * PEAK[lit] * ramp).round() as u16
                } else {
                    LEAK
                };
                data.push(value);
            }
        }
        Ok(RawLinear {
            width: WIDTH,
            height: HEIGHT,
            cfa: Cfa::RGGB,
            black: [0.0; 4],
            white: WHITE,
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gtp_capture::CameraSource;
    use gtp_io::demosaic_channel;

    use super::*;

    #[test]
    fn each_channel_is_bright_in_its_own_exposure_only_and_never_clipped() {
        for (prefix, lit) in [("r", 0), ("g", 1), ("b", 2)] {
            let raw = FakeRawDecoder
                .decode(
                    Path::new(&format!("{prefix}-FAKE_0001.NEF")),
                    &Limits::default(),
                )
                .unwrap();
            let mean = |channel: usize| {
                let plane = demosaic_channel(&raw, channel).unwrap();
                plane.data.iter().sum::<f32>() / plane.data.len() as f32
            };
            for other in (0..3).filter(|c| *c != lit) {
                assert!(mean(lit) > 5.0 * mean(other), "{prefix}: {lit} vs {other}");
            }
            let max = demosaic_channel(&raw, lit)
                .unwrap()
                .data
                .into_iter()
                .fold(0.0f32, f32::max);
            assert!(max < 0.99, "{prefix} clips at {max}");
        }
    }

    #[test]
    fn a_file_that_is_not_a_fake_picture_is_refused_and_the_camera_never_runs_out() {
        let err = FakeRawDecoder
            .decode(Path::new("x-1.nef"), &Limits::default())
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::UnsupportedFormat);
        let mut camera = fake_camera();
        for _ in 0..10 {
            let file = camera.wait_for_image(Duration::from_secs(1)).unwrap();
            assert!(file.name.ends_with(".NEF"));
        }
    }
}
