//! The roll preview (H-12): the scanner's framing thumbnail of the whole roll, turned into a
//! positive for the contact sheet. The scanner shows the negative as it lies; the film base, the
//! inversion and the levels are the ones the pipeline uses for any frame, found automatically on the
//! strip. The frames are not cut apart here: the strip comes back whole with the pitch the scanner's
//! frames are evenly spaced at, so the view can lay a grid over it and the grid can be moved (H-13).

use std::sync::{Mutex, PoisonError};

use gtp_core::recipe::{Crop, OutputSpace};
use gtp_core::{AppError, ErrorCode};
use gtp_ingest::{CancelToken, RollPreview, ScannerSource};
use gtp_io::{encode_jpeg_bytes, to_f32_linear, Tiff16};
use serde::Serialize;

use crate::pipeline::{analyze_with, render, PipelineInput, RenderTarget};

/// The JPEG quality of the strip.
pub const STRIP_JPEG_QUALITY: u8 = 85;

/// The roll as the contact sheet shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RollPreviewImage {
    /// How many frames the scanner found.
    pub frame_count: u32,
    /// The distance between the starts of two frames on the strip if they are evenly spaced: the
    /// strip's width divided by the frame count, in strip pixels.
    pub pitch: f32,
    pub strip_width: u32,
    pub strip_height: u32,
    /// The strip as a positive, sRGB, JPEG.
    pub strip_jpeg: Vec<u8>,
}

/// Turns the scanner's strip into a positive JPEG with its pitch. `InvalidInput` for a strip with
/// no pixels or a frame count of 0.
pub fn make_preview(preview: &RollPreview) -> Result<RollPreviewImage, AppError> {
    let strip = &preview.strip;
    let samples = (strip.width as usize) * (strip.height as usize) * 3;
    if strip.width == 0 || strip.height == 0 || strip.rgb16.len() != samples {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The roll thumbnail is empty or damaged.",
        )
        .with_details(format!("{} x {}", strip.width, strip.height)));
    }
    if preview.frame_count == 0 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The scanner found no frames on the roll.",
        ));
    }
    let linear = to_f32_linear(&Tiff16 {
        width: strip.width,
        height: strip.height,
        channels: 3,
        data: strip.rgb16.clone(),
    })?;
    let input = PipelineInput {
        rgb: std::sync::Arc::new(linear),
        ir: None,
        film_base: None,
    };
    // The automatic recipe over the whole strip: its base and levels, but no crop (the strip is the
    // whole roll, rebate and all).
    let mut recipe = analyze_with(&input, None)?.auto_recipe;
    recipe.geometry.crop = Crop::default();
    let image = render(
        &input,
        &recipe,
        RenderTarget::Preview {
            max_edge: strip.width.max(strip.height),
        },
    )?;
    let strip_jpeg = encode_jpeg_bytes(&image, OutputSpace::Srgb, STRIP_JPEG_QUALITY)?;
    Ok(RollPreviewImage {
        frame_count: preview.frame_count,
        pitch: strip.width as f32 / preview.frame_count as f32,
        strip_width: image.width,
        strip_height: image.height,
        strip_jpeg,
    })
}

/// Runs one preview at a time and can stop it.
#[derive(Default)]
pub struct ScanPreviewService {
    running: Mutex<Option<CancelToken>>,
}

impl ScanPreviewService {
    /// Previews the roll in `device_id`. `ScannerUnavailable` when a preview is already running.
    pub fn run(
        &self,
        scanner: &dyn ScannerSource,
        device_id: &str,
    ) -> Result<RollPreviewImage, AppError> {
        let cancel = CancelToken::default();
        {
            let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
            if running.is_some() {
                return Err(AppError::new(
                    ErrorCode::ScannerUnavailable,
                    "A roll preview is already running.",
                ));
            }
            *running = Some(cancel.clone());
        }
        let result = scanner
            .preview(device_id, &cancel)
            .and_then(|preview| make_preview(&preview));
        *self.running.lock().unwrap_or_else(PoisonError::into_inner) = None;
        result
    }

    /// True while a preview runs.
    pub fn is_running(&self) -> bool {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Stops the running preview; false when none is running.
    pub fn cancel(&self) -> bool {
        match &*self.running.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use gtp_ingest::{RollStrip, ScanParams, FIXTURE_STRIP_HEIGHT};

    use super::*;

    /// A strip of `frames` negative-looking frames with orange rebate between them: bright rebate
    /// and darker, varied frames, as the scanner shows a roll.
    fn strip(frames: u32) -> RollPreview {
        let (fw, h) = (40u32, 24u32);
        let (w, mut data) = (frames * fw, Vec::new());
        for y in 0..h {
            for x in 0..w {
                let in_gap = x % fw < 4 || y < 2 || y >= h - 2;
                let base = [52_000u16, 36_000, 20_000];
                let pixel = if in_gap {
                    base
                } else {
                    let v = 0.35 + 0.4 * (((x * 7 + y * 3 + (x / fw) * 11) % 17) as f32 / 17.0);
                    base.map(|b| (f32::from(b) * v) as u16)
                };
                data.extend(pixel);
            }
        }
        RollPreview {
            frame_count: frames,
            strip: RollStrip {
                width: w,
                height: h,
                rgb16: data,
            },
        }
    }

    fn decode(jpeg: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut decoder = jpeg_decoder::Decoder::new(jpeg);
        let pixels = decoder.decode().unwrap();
        let info = decoder.info().unwrap();
        (u32::from(info.width), u32::from(info.height), pixels)
    }

    #[test]
    fn the_preview_is_the_whole_strip_as_a_positive_with_its_pitch() {
        let made = make_preview(&strip(6)).unwrap();
        assert_eq!(made.frame_count, 6);
        assert_eq!((made.strip_width, made.strip_height), (240, 24));
        assert_eq!(made.pitch, 40.0);
        let (w, h, pixels) = decode(&made.strip_jpeg);
        assert_eq!((w, h), (240, 24));
        // A negative's bright rebate is dark in the positive; its frames are lighter than it.
        let at = |x: usize, y: usize| {
            let i = (y * w as usize + x) * 3;
            f32::from(pixels[i]) + f32::from(pixels[i + 1]) + f32::from(pixels[i + 2])
        };
        assert!(
            at(1, 12) < at(20, 12),
            "{} against {}",
            at(1, 12),
            at(20, 12)
        );
    }

    #[test]
    fn a_pitch_that_does_not_divide_the_width_is_fractional() {
        let mut odd = strip(5);
        odd.frame_count = 7;
        let made = make_preview(&odd).unwrap();
        assert!((made.pitch - 200.0 / 7.0).abs() < 1e-4, "{}", made.pitch);
    }

    #[test]
    fn a_damaged_strip_or_no_frames_is_invalid_input() {
        let mut bad = strip(3);
        bad.strip.rgb16.pop();
        assert_eq!(
            make_preview(&bad).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let mut empty = strip(3);
        empty.frame_count = 0;
        assert_eq!(
            make_preview(&empty).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let mut flat = strip(3);
        flat.strip.height = 0;
        assert_eq!(
            make_preview(&flat).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    /// A scanner whose preview waits for a cancel.
    struct Waiting;

    impl ScannerSource for Waiting {
        fn list_devices(&self) -> Result<Vec<gtp_ingest::DeviceInfo>, AppError> {
            Ok(Vec::new())
        }
        fn scan(
            &self,
            _: &ScanParams,
            _: &mut dyn FnMut(u32, gtp_ingest::RawScan) -> Result<(), AppError>,
            _: &dyn Fn(f32),
            _: &CancelToken,
        ) -> Result<(), AppError> {
            Ok(())
        }
        fn preview(&self, _: &str, cancel: &CancelToken) -> Result<RollPreview, AppError> {
            while !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(10));
            }
            cancel.check().map(|()| strip(1))
        }
    }

    #[test]
    fn one_preview_runs_at_a_time_and_can_be_cancelled() {
        let service = Arc::new(ScanPreviewService::default());
        assert!(!service.cancel(), "nothing to cancel");
        let background = Arc::clone(&service);
        let running = std::thread::spawn(move || background.run(&Waiting, "d"));
        while !service.cancel() {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            running.join().unwrap().unwrap_err().code,
            ErrorCode::Cancelled
        );
        // It is free again afterwards.
        assert!(!service.cancel());
    }

    #[test]
    fn a_second_preview_while_one_runs_is_refused() {
        let service = Arc::new(ScanPreviewService::default());
        let background = Arc::clone(&service);
        let first = std::thread::spawn(move || background.run(&Waiting, "d"));
        std::thread::sleep(Duration::from_millis(100));
        let err = service.run(&Waiting, "d").unwrap_err();
        assert_eq!(err.code, ErrorCode::ScannerUnavailable);
        service.cancel();
        let _ = first.join();
    }

    #[test]
    fn a_fixture_roll_previews_through_the_service() {
        let dir = std::env::temp_dir().join(format!("gtp-preview-fixture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in 1..=3u32 {
            let tiff = Tiff16 {
                width: 60,
                height: 40,
                channels: 3,
                data: (0..60 * 40 * 3)
                    .map(|i| 10_000 + ((i * 37 + n * 1000) % 30_000) as u16)
                    .collect(),
            };
            gtp_io::write_tiff16(
                &dir.join(format!("{n:04}.tif")),
                &tiff,
                gtp_io::Compression::None,
            )
            .unwrap();
        }
        let source = gtp_ingest::FixtureSource::new(&dir);
        let made = ScanPreviewService::default()
            .run(&source, gtp_ingest::FIXTURE_DEVICE_ID)
            .unwrap();
        assert_eq!(made.frame_count, 3);
        assert_eq!(made.strip_height, FIXTURE_STRIP_HEIGHT);
        assert_eq!(made.pitch, 144.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Previews a real thumbnail: `GTP_PREVIEW_STRIP=<tiff> GTP_PREVIEW_FRAMES=36`; writes the
    /// result to `roll-preview.jpg` in the temporary folder and prints where.
    #[test]
    #[ignore = "needs a thumbnail written by nkscan"]
    fn a_real_thumbnail_becomes_a_positive_strip() {
        let (Some(path), Some(frames)) = (
            std::env::var_os("GTP_PREVIEW_STRIP"),
            std::env::var("GTP_PREVIEW_FRAMES")
                .ok()
                .and_then(|f| f.parse::<u32>().ok()),
        ) else {
            eprintln!("set GTP_PREVIEW_STRIP and GTP_PREVIEW_FRAMES; skipped");
            return;
        };
        let tiff =
            gtp_io::read_tiff16(std::path::Path::new(&path), &gtp_io::Limits::default()).unwrap();
        let made = make_preview(&RollPreview {
            frame_count: frames,
            strip: RollStrip {
                width: tiff.width,
                height: tiff.height,
                rgb16: tiff.data,
            },
        })
        .unwrap();
        let out = std::env::temp_dir().join("roll-preview.jpg");
        std::fs::write(&out, &made.strip_jpeg).unwrap();
        eprintln!("{} (pitch {:.1})", out.display(), made.pitch);
    }
}
