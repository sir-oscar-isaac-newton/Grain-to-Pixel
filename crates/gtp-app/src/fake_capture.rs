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
use gtp_capture::{CameraInfo, CaptureProfile, CapturedFile, FakeCamera};
pub use gtp_capture::{FakeLight, FakeLightHandle, LightInfo, LightSource, LightStatus};
use gtp_core::recipe::OutputSpace;
use gtp_core::{AppError, ErrorCode, ImageF32};
use gtp_io::{encode_jpeg_bytes, Cfa, Limits, RawDecoder, RawLinear};

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

/// A fake camera that delivers the same small picture for every shot and serves live-view frames
/// ([`fake_preview_frame`]).
pub fn fake_camera() -> FakeCamera {
    FakeCamera::new(Vec::new())
        .repeating(CapturedFile {
            name: "FAKE_0001.NEF".to_owned(),
            bytes: b"grain to pixel fake picture".to_vec(),
        })
        .with_frames(fake_preview_frame)
}

/// The size of a live-view frame of the D600 (and of the fake's).
pub const PREVIEW_WIDTH: u32 = 640;
/// See [`PREVIEW_WIDTH`].
pub const PREVIEW_HEIGHT: u32 = 424;
/// How far the bar of a fake frame moves from one frame to the next, in pixels.
const BAR_STEP_PX: usize = 16;
const BAR_WIDTH_PX: usize = 24;
/// The counter at the top of a frame: this many squares, one per bit of the frame number.
const COUNTER_BITS: usize = 16;
const COUNTER_SQUARE_PX: usize = 20;

/// Frame number `n` of the fake live view, as JPEG bytes: a grey gradient with a bright bar that
/// moves right (and wraps) by a few pixels each frame, and at the top a row of squares that show
/// `n` in binary, so that every frame differs and the view visibly updates.
pub fn fake_preview_frame(n: usize) -> Result<Vec<u8>, AppError> {
    let (w, h) = (PREVIEW_WIDTH as usize, PREVIEW_HEIGHT as usize);
    let bar_x = (n * BAR_STEP_PX) % w;
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let counter_bit = (y < COUNTER_SQUARE_PX && x < COUNTER_BITS * COUNTER_SQUARE_PX)
                .then_some((x / COUNTER_SQUARE_PX, x % COUNTER_SQUARE_PX));
            let value = if let Some((bit, within)) = counter_bit {
                if within < 2 || (n >> bit) & 1 == 1 {
                    0.9
                } else {
                    0.05
                }
            } else if (x + w - bar_x) % w < BAR_WIDTH_PX {
                0.8
            } else {
                0.05 + 0.25 * (x + y) as f32 / (w + h) as f32
            };
            data.extend_from_slice(&[value, value, value]);
        }
    }
    let image = ImageF32::new(PREVIEW_WIDTH, PREVIEW_HEIGHT, 3, data)?;
    encode_jpeg_bytes(&image, OutputSpace::Srgb, 80)
}

/// Reads the staged pictures of the fake camera: the channel comes from the first letter of the
/// file name (`r`, `g` or `b`). The lit colour is as bright as at the default levels.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeRawDecoder;

impl RawDecoder for FakeRawDecoder {
    fn decode(&self, path: &Path, _: &Limits) -> Result<RawLinear, AppError> {
        decode_lit(path, 1.0)
    }
}

/// [`FakeRawDecoder`] whose lit colour follows the light: as bright as at the default levels when
/// the light was at its default level for that colour, twice as bright at twice the level, up to
/// the white level. It reads the level the fake light last had for the colour, so a calibration
/// against the fake hardware converges.
#[derive(Clone)]
pub struct LevelledRawDecoder {
    light: FakeLightHandle,
}

impl LevelledRawDecoder {
    /// A decoder that follows `light`.
    pub fn new(light: FakeLightHandle) -> Self {
        Self { light }
    }
}

impl RawDecoder for LevelledRawDecoder {
    fn decode(&self, path: &Path, _: &Limits) -> Result<RawLinear, AppError> {
        let lit = lit_channel(path)?;
        let level = self.light.last_lit_level(lit);
        // A light that was never lit leaves the default brightness.
        let scale = if level == 0 {
            1.0
        } else {
            f32::from(level) / f32::from(CaptureProfile::default().levels[lit])
        };
        decode_lit(path, scale)
    }
}

/// The colour a staged fake picture was lit with: 0, 1 or 2.
fn lit_channel(path: &Path) -> Result<usize, AppError> {
    match path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.chars().next())
    {
        Some('r') => Ok(0),
        Some('g') => Ok(1),
        Some('b') => Ok(2),
        _ => Err(AppError::new(
            ErrorCode::UnsupportedFormat,
            "This is not a picture of the fake camera.",
        )),
    }
}

/// The synthetic mosaic of the picture at `path`, its lit colour `scale` times as bright as at the
/// default level (never above the white level).
fn decode_lit(path: &Path, scale: f32) -> Result<RawLinear, AppError> {
    let lit = lit_channel(path)?;
    let (w, h) = (WIDTH as usize, HEIGHT as usize);
    let mut data = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let value = if Cfa::RGGB.color_at(x, y) == lit {
                // A smooth ramp from 30 % to 100 % of the peak, left to right and top to bottom.
                let ramp = 0.3 + 0.7 * ((x + y) as f32 / (w + h - 2) as f32);
                (WHITE * (PEAK[lit] * ramp * scale).min(1.0)).round() as u16
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
    fn a_brighter_light_level_gives_a_brighter_lit_channel_and_the_default_changes_nothing() {
        let mut light = gtp_capture::FakeLight::new();
        let decoder = LevelledRawDecoder::new(light.handle());
        let peak = |name: &str, lit: usize| {
            let raw = decoder.decode(Path::new(name), &Limits::default()).unwrap();
            demosaic_channel(&raw, lit)
                .unwrap()
                .data
                .into_iter()
                .fold(0.0f32, f32::max)
        };
        let default = CaptureProfile::default().levels;
        for (prefix, lit) in [("r", 0), ("g", 1)] {
            let name = format!("{prefix}-FAKE_0001.NEF");
            let plain = FakeRawDecoder
                .decode(Path::new(&name), &Limits::default())
                .unwrap();
            let plain_max = demosaic_channel(&plain, lit)
                .unwrap()
                .data
                .into_iter()
                .fold(0.0f32, f32::max);
            assert_eq!(peak(&name, lit), plain_max, "never lit: as before");
            let mut levels = [0u8; 3];
            levels[lit] = default[lit];
            light.set_levels(levels).unwrap();
            assert_eq!(peak(&name, lit), plain_max, "{prefix}: the default level");
            levels[lit] = default[lit] / 2;
            light.set_levels(levels).unwrap();
            light.set_levels([0; 3]).unwrap();
            let dimmer = peak(&name, lit);
            assert!(
                (dimmer - plain_max / 2.0).abs() < 0.01,
                "{prefix}: {dimmer}"
            );
            levels[lit] = default[lit] + default[lit] / 4;
            light.set_levels(levels).unwrap();
            assert!(peak(&name, lit) > plain_max * 1.2, "{prefix}");
        }
        light.set_levels([255, 0, 0]).unwrap();
        assert!(peak("r-FAKE_0001.NEF", 0) <= 1.0, "clipped at white");
    }

    #[test]
    fn the_fake_live_view_frames_are_640_by_424_jpegs_that_differ_from_frame_to_frame() {
        let frames: Vec<Vec<u8>> = (0..4).map(|n| fake_preview_frame(n).unwrap()).collect();
        for frame in &frames {
            assert_eq!(frame[..2], [0xFF, 0xD8]);
            assert_eq!(frame[frame.len() - 2..], [0xFF, 0xD9]);
            let mut decoder = jpeg_decoder::Decoder::new(frame.as_slice());
            let pixels = decoder.decode().unwrap();
            let info = decoder.info().unwrap();
            assert_eq!((info.width, info.height), (640, 424));
            assert_eq!(pixels.len(), 640 * 424 * 3);
        }
        for pair in frames.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
        assert_ne!(frames[0], fake_preview_frame(40).unwrap(), "the bar wraps");
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
