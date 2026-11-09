//! The capture devices of `GTP_FAKE_HW=1`: a [`FakeLight`], a [`FakeCamera`] that never runs out of
//! pictures, and the decoder that reads those pictures, so the capture commands and the capture
//! job run end to end without hardware.
//!
//! The camera's picture is a few bytes with a camera-like name: it has no sensor data. The
//! capture job stages each picture in a file named for its channel (`r-`, `g-`, `b-`), and
//! [`FakeRawDecoder`] answers with a deterministic synthetic mosaic for that channel: the lit
//! colour has a gradient well below the white level (no clipping), the others a dark leak, as in
//! a real exposure under one LED. The lit colour also carries a fine grain, sharp in the middle and
//! a little softer towards the edges, so the focus check (H-19) has a crop and a map to show (a
//! mild bowl).

use std::path::Path;

// Re-exported so that the tests of the desktop shell can script a light without a dependency on
// `gtp-capture` (the shell depends on `gtp-app` only).
use gtp_capture::{CameraInfo, CaptureProfile, CapturedFile, FakeCamera, PreviewView};
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
/// The grain of the lit colour: its size as a fraction of the brightness at the middle, and how
/// much of it is lost at the corners (the grain scales the brightness, so it adds no mean).
const GRAIN: f32 = 0.04;
const GRAIN_LOSS_AT_CORNER: f32 = 0.3;
/// The width of a grain clump, in photosites.
const GRAIN_CLUMP: usize = 2;

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

/// How many times the fake magnifies its picture at a zoom of 100 %; the other steps in
/// proportion (25 % a quarter of it, 200 % twice), roughly what the D600's 640 px frame shows of
/// its 6016 px sensor.
const MAGNIFY_AT_100: f32 = 9.0;
/// The squares at the bottom of a zoomed frame, one per 25 % of zoom, that tell the step.
const ZOOM_SQUARE_PX: usize = 20;
/// The grid of the fake picture, in picture pixels, so that magnification shows.
const GRID_PX: f32 = 16.0;

/// The zoom as a percentage (`"100%"` is 100); 0 for the whole frame or a choice that is none.
fn zoom_percent(zoom: &str) -> f32 {
    zoom.trim().trim_end_matches('%').parse().unwrap_or(0.0)
}

/// How many times `zoom` magnifies the frame: 1 for the whole frame.
fn magnification(zoom: &str) -> f32 {
    (zoom_percent(zoom) / 100.0 * MAGNIFY_AT_100).max(1.0)
}

/// The brightness of the fake picture at picture point (`sx`, `sy`): a grey gradient, a faint
/// grid, and a bright bar that starts at `bar_x`.
fn picture_value(sx: f32, sy: f32, bar_x: f32) -> f32 {
    let (w, h) = (PREVIEW_WIDTH as f32, PREVIEW_HEIGHT as f32);
    if (sx + w - bar_x).rem_euclid(w) < BAR_WIDTH_PX as f32 {
        return 0.8;
    }
    let grid = if sx.rem_euclid(GRID_PX) < 1.0 || sy.rem_euclid(GRID_PX) < 1.0 {
        0.1
    } else {
        0.0
    };
    0.05 + 0.25 * (sx.max(0.0) + sy.max(0.0)) / (w + h) + grid
}

/// Frame number `n` of the fake live view, as JPEG bytes: a grey gradient with a faint grid and a
/// bright bar that moves right (and wraps) by a few pixels each frame, and at the top a row of
/// squares that show `n` in binary, so that every frame differs and the view visibly updates.
/// When `view` is zoomed, the picture is magnified around the area (the middle until one is set),
/// and a row of squares at the bottom, one per 25 % of the zoom, tells the step.
pub fn fake_preview_frame(n: usize, view: &PreviewView) -> Result<Vec<u8>, AppError> {
    let (w, h) = (PREVIEW_WIDTH as usize, PREVIEW_HEIGHT as usize);
    let bar_x = ((n * BAR_STEP_PX) % w) as f32;
    let factor = magnification(&view.zoom);
    let zoomed = factor > 1.0;
    let (ax, ay) = view.area.unwrap_or((0.5, 0.5));
    let (centre_x, centre_y) = (ax * w as f32, ay * h as f32);
    let zoom_squares = if zoomed {
        (zoom_percent(&view.zoom) / 25.0) as usize
    } else {
        0
    };
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
            } else if y >= h - ZOOM_SQUARE_PX && x < zoom_squares * ZOOM_SQUARE_PX {
                0.95
            } else if zoomed {
                let sx = centre_x + (x as f32 - w as f32 / 2.0) / factor;
                let sy = centre_y + (y as f32 - h as f32 / 2.0) / factor;
                picture_value(sx, sy, bar_x)
            } else {
                picture_value(x as f32, y as f32, bar_x)
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
                // Grain clumps of GRAIN_CLUMP photosites, as real grain spans a few at 1:1 (the
                // focus check measures on 2 x 2 averages, which smooth single-photosite noise).
                let grain =
                    GRAIN * grain_sharpness(x, y) * grain_noise(x / GRAIN_CLUMP, y / GRAIN_CLUMP);
                (WHITE * (PEAK[lit] * ramp * scale * (1.0 + grain)).min(1.0)).round() as u16
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

/// How much of the grain is kept at photosite (`x`, `y`): all of it at the middle, less towards
/// the edges (`GRAIN_LOSS_AT_CORNER` lost at a corner).
fn grain_sharpness(x: usize, y: usize) -> f32 {
    let dx = 2.0 * x as f32 / (WIDTH - 1) as f32 - 1.0;
    let dy = 2.0 * y as f32 / (HEIGHT - 1) as f32 - 1.0;
    1.0 - GRAIN_LOSS_AT_CORNER * (dx * dx + dy * dy) / 2.0
}

/// Deterministic noise from -1 to 1 for photosite (`x`, `y`).
fn grain_noise(x: usize, y: usize) -> f32 {
    // Both coordinates mixed in with their own odd constants, then the murmur3 finalizer (the
    // finalizer on the plain photosite number left rows of clumps alike).
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    h as f32 / u32::MAX as f32 * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gtp_capture::{CameraPreview, CameraSource};
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
        let frames: Vec<Vec<u8>> = (0..4)
            .map(|n| fake_preview_frame(n, &whole()).unwrap())
            .collect();
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
        assert_ne!(
            frames[0],
            fake_preview_frame(40, &whole()).unwrap(),
            "the bar wraps"
        );
    }

    fn whole() -> PreviewView {
        PreviewView {
            zoom: "Entire Display".to_owned(),
            area: None,
        }
    }

    /// The grey values of a frame, three channels to one.
    fn grey(frame: &[u8]) -> Vec<u8> {
        let pixels = jpeg_decoder::Decoder::new(frame).decode().unwrap();
        pixels.chunks(3).map(|p| p[0]).collect()
    }

    #[test]
    fn a_zoomed_frame_is_magnified_around_the_area_and_labelled() {
        let frame = |zoom: &str, area| {
            let view = PreviewView {
                zoom: zoom.to_owned(),
                area,
            };
            grey(&fake_preview_frame(20, &view).unwrap())
        };
        let whole_frame = frame("Entire Display", None);
        let at_100 = frame("100%", None);
        assert_ne!(whole_frame, at_100);
        assert_ne!(at_100, frame("200%", None), "each step differs");
        assert_ne!(
            at_100,
            frame("100%", Some((0.1, 0.9))),
            "the area moves the view"
        );
        // Magnified: the grid lines of the picture are further apart on screen, so a row crosses
        // fewer edges than at the whole frame.
        let w = PREVIEW_WIDTH as usize;
        let row = PREVIEW_HEIGHT as usize / 2;
        let edges = |g: &[u8]| {
            g[row * w + 100..row * w + 500]
                .windows(2)
                .filter(|p| p[0].abs_diff(p[1]) > 8)
                .count()
        };
        assert!(edges(&at_100) < edges(&whole_frame));
        // The label: one square per 25 % at the bottom left.
        let bottom = (PREVIEW_HEIGHT as usize - 5) * w;
        let bright = |g: &[u8]| g[bottom..bottom + 200].iter().filter(|v| **v > 200).count();
        assert_eq!(bright(&whole_frame), 0);
        assert!(bright(&frame("200%", None)) > bright(&at_100));
    }

    #[test]
    fn the_zoom_choices_of_the_fake_camera_magnify_in_proportion() {
        assert_eq!(magnification("Entire Display"), 1.0);
        assert_eq!(magnification("25%"), 2.25);
        assert_eq!(magnification("200%"), 2.0 * magnification("100%"));
        assert_eq!(magnification("nonsense"), 1.0);
    }

    #[test]
    fn the_fake_camera_hands_its_view_to_the_frames() {
        let camera = fake_camera();
        let handle = camera.handle();
        let whole_frame = handle.preview_frame().unwrap();
        handle.set_preview_zoom("100%").unwrap();
        handle.set_preview_area(0.2, 0.8).unwrap();
        assert_ne!(handle.preview_frame().unwrap(), whole_frame);
        handle.preview_stop().unwrap();
        assert_eq!(grey(&handle.preview_frame().unwrap()).len(), 640 * 424);
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
