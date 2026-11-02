//! A scanner that replays TIFF files, so scan jobs can be tested and the app run without the
//! LS-50 (`GTP_FAKE_HW=1`). Frame `n` of a scan is the `n`-th TIFF of the folder in file-name
//! order, so a scan that resumes at frame 5 gets the fifth file, as a real roll would.
//!
//! A scan with a separate `<name>_IR.tif` file or a 4-channel RGBI file also gets its infrared
//! plane; without either the frame has none.

use std::fs;
use std::path::{Path, PathBuf};

use gtp_core::{AppError, ErrorCode};
use gtp_io::{read_tiff16, split_rgbi, Limits, Tiff16};

use crate::files::{scan_tiff_candidates, TiffCandidate};
use crate::scanner::{CancelToken, DeviceInfo, RawScan, ScanMeta, ScanParams, ScannerSource};

/// The id of the one device a [`FixtureSource`] lists.
pub const FIXTURE_DEVICE_ID: &str = "fixture";

/// Replays the TIFF files of a folder as frames of a roll.
#[derive(Debug, Clone)]
pub struct FixtureSource {
    dir: PathBuf,
}

fn unavailable(message: &str, details: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::ScannerUnavailable, message).with_details(details)
}

impl FixtureSource {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The folder the frames come from.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The frames of the folder in file-name order.
    fn frames(&self) -> Result<Vec<TiffCandidate>, AppError> {
        let entries = fs::read_dir(&self.dir).map_err(|err| {
            unavailable(
                "The fixture scanner has no folder of scans to replay.",
                format!("{}: {err}", self.dir.display()),
            )
        })?;
        let paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        Ok(scan_tiff_candidates(&paths).candidates)
    }

    /// Reads one frame; the infrared plane only when `params.ir` asks for it.
    fn load(&self, candidate: &TiffCandidate, p: &ScanParams) -> Result<RawScan, AppError> {
        let failed = |message: &str, details: String| {
            AppError::new(ErrorCode::ScanFailed, message).with_details(details)
        };
        let limits = Limits::default();
        let image = read_tiff16(&candidate.scan, &limits)?;
        let (rgb, mut ir): (Tiff16, Option<Tiff16>) = match image.channels {
            3 => (image, None),
            4 => {
                let (rgb, ir) = split_rgbi(&image)?;
                (rgb, Some(ir))
            }
            channels => {
                return Err(failed(
                    "A fixture scan must be a colour TIFF.",
                    format!("{}: {channels} channels", candidate.scan.display()),
                ))
            }
        };
        if ir.is_none() {
            if let Some(path) = &candidate.ir {
                let plane = read_tiff16(path, &limits)?;
                if plane.channels != 1 || (plane.width, plane.height) != (rgb.width, rgb.height) {
                    return Err(failed(
                        "The infrared file does not match its scan.",
                        path.display().to_string(),
                    ));
                }
                ir = Some(plane);
            }
        }
        let ir16 = if p.ir {
            ir.map(|plane| plane.data)
        } else {
            None
        };
        Ok(RawScan {
            width: rgb.width,
            height: rgb.height,
            rgb16: rgb.data,
            ir16,
            dpi: p.dpi,
            meta: ScanMeta {
                device_id: p.device_id.clone(),
                samples: p.samples,
                autofocus: p.autofocus,
            },
        })
    }
}

impl ScannerSource for FixtureSource {
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AppError> {
        let frames = self.frames()?.len();
        Ok(vec![DeviceInfo {
            id: FIXTURE_DEVICE_ID.to_owned(),
            name: format!("Fixture scanner ({frames} frames)"),
        }])
    }

    fn scan(
        &self,
        p: &ScanParams,
        on_frame: &mut dyn FnMut(u32, RawScan) -> Result<(), AppError>,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<(), AppError> {
        p.validate()?;
        if p.device_id != FIXTURE_DEVICE_ID {
            return Err(unavailable(
                "That scanner is not connected.",
                p.device_id.clone(),
            ));
        }
        let available = self.frames()?;
        let total = p.frame_count() as f32;
        progress(0.0);
        for (done, frame) in p.frames.clone().enumerate() {
            cancel.check()?;
            let Some(candidate) = available.get(frame as usize - 1) else {
                return Err(AppError::new(
                    ErrorCode::ScanFailed,
                    format!(
                        "The fixture folder has only {} scans; frame {frame} is missing.",
                        available.len()
                    ),
                )
                .with_details(self.dir.display().to_string()));
            };
            on_frame(frame, self.load(candidate, p)?)?;
            progress((done + 1) as f32 / total);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gtp_core::testing::synthetic::{self, Params, Pattern};
    use gtp_core::FrameId;
    use gtp_io::{from_f32_linear, merge_rgbi, write_tiff16, Compression};

    use super::*;

    const W: u32 = 60;
    const H: u32 = 40;

    /// A folder of synthetic scans: `0001.tif` plain RGB, `0002.tif` RGBI (4 channels), `0003.tif`
    /// RGB with a separate `0003_IR.tif`.
    struct Roll {
        dir: PathBuf,
    }

    impl Roll {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "gtp-fixture-{label}-{}-{}",
                std::process::id(),
                FrameId::new()
            ));
            fs::create_dir_all(&dir).unwrap();
            let roll = Self { dir };
            roll.scan("0001.tif", 1, false);
            roll.scan("0002.tif", 2, true);
            roll.scan("0003.tif", 3, false);
            let ir = Tiff16 {
                width: W,
                height: H,
                channels: 1,
                data: vec![40_000; (W * H) as usize],
            };
            write_tiff16(&roll.dir.join("0003_IR.tif"), &ir, Compression::None).unwrap();
            roll
        }

        fn scan(&self, name: &str, seed: u64, rgbi: bool) {
            let s = synthetic::negative(
                seed,
                &Params {
                    width: W,
                    height: H,
                    rebate: [4; 4],
                    pattern: Pattern::Blobs,
                    with_ir: rgbi,
                    ..Params::default()
                },
            )
            .unwrap();
            let rgb = from_f32_linear(&s.image);
            let tiff = match &s.ir {
                Some(ir) => merge_rgbi(&rgb, &from_f32_linear(ir)).unwrap(),
                None => rgb,
            };
            write_tiff16(&self.dir.join(name), &tiff, Compression::None).unwrap();
        }

        fn source(&self) -> FixtureSource {
            FixtureSource::new(&self.dir)
        }
    }

    impl Drop for Roll {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn params(frames: std::ops::RangeInclusive<u32>) -> ScanParams {
        ScanParams::new(FIXTURE_DEVICE_ID, frames)
    }

    /// Runs a scan and returns what arrived: the frame numbers, the scans, the progress values and
    /// the result.
    struct Run {
        frames: Vec<(u32, RawScan)>,
        progress: Vec<f32>,
        result: Result<(), AppError>,
    }

    fn run(source: &FixtureSource, p: &ScanParams, cancel: &CancelToken) -> Run {
        let frames = RefCell::new(Vec::new());
        let progress = RefCell::new(Vec::new());
        let result = source.scan(
            p,
            &mut |n, scan| {
                frames.borrow_mut().push((n, scan));
                Ok(())
            },
            &|f| progress.borrow_mut().push(f),
            cancel,
        );
        Run {
            frames: frames.into_inner(),
            progress: progress.into_inner(),
            result,
        }
    }

    fn numbers(run: &Run) -> Vec<u32> {
        run.frames.iter().map(|(n, _)| *n).collect()
    }

    #[test]
    fn frames_arrive_in_order_as_the_files_of_the_folder() {
        let roll = Roll::new("order");
        let out = run(&roll.source(), &params(1..=3), &CancelToken::default());
        assert!(out.result.is_ok());
        assert_eq!(numbers(&out), [1, 2, 3]);
        for (_, scan) in &out.frames {
            assert_eq!((scan.width, scan.height), (W, H));
            assert_eq!(scan.rgb16.len(), (W * H * 3) as usize);
            assert_eq!(scan.dpi, 4000);
            assert_eq!(scan.meta.device_id, FIXTURE_DEVICE_ID);
        }
        // Frame 1 is exactly the first file.
        let first = read_tiff16(&roll.dir.join("0001.tif"), &Limits::default()).unwrap();
        assert_eq!(out.frames[0].1.rgb16, first.data);
        assert_ne!(out.frames[0].1.rgb16, out.frames[1].1.rgb16);
    }

    #[test]
    fn the_infrared_plane_comes_from_a_fourth_channel_or_a_separate_file() {
        let roll = Roll::new("ir");
        let out = run(&roll.source(), &params(1..=3), &CancelToken::default());
        let ir: Vec<Option<usize>> = out
            .frames
            .iter()
            .map(|(_, s)| s.ir16.as_ref().map(Vec::len))
            .collect();
        let plane = Some((W * H) as usize);
        assert_eq!(ir, [None, plane, plane], "plain, RGBI, separate file");
        assert!(out.frames[2]
            .1
            .ir16
            .as_ref()
            .unwrap()
            .iter()
            .all(|v| *v == 40_000));

        let without = ScanParams {
            ir: false,
            ..params(1..=3)
        };
        let out = run(&roll.source(), &without, &CancelToken::default());
        assert!(
            out.frames.iter().all(|(_, s)| s.ir16.is_none()),
            "ir off drops the plane"
        );
    }

    #[test]
    fn a_scan_can_resume_at_a_later_frame() {
        let roll = Roll::new("resume");
        let out = run(&roll.source(), &params(2..=3), &CancelToken::default());
        assert_eq!(numbers(&out), [2, 3]);
        let second = read_tiff16(&roll.dir.join("0002.tif"), &Limits::default()).unwrap();
        let (rgb, _) = split_rgbi(&second).unwrap();
        assert_eq!(out.frames[0].1.rgb16, rgb.data);
    }

    #[test]
    fn progress_only_moves_forward_and_ends_at_one() {
        let roll = Roll::new("progress");
        let out = run(&roll.source(), &params(1..=3), &CancelToken::default());
        assert_eq!(out.progress.first(), Some(&0.0));
        assert_eq!(out.progress.last(), Some(&1.0));
        assert!(out.progress.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn asking_for_more_frames_than_the_folder_has_keeps_the_ones_that_exist() {
        let roll = Roll::new("short");
        let out = run(&roll.source(), &params(1..=5), &CancelToken::default());
        assert_eq!(
            numbers(&out),
            [1, 2, 3],
            "frames before the missing one were delivered"
        );
        let err = out.result.unwrap_err();
        assert_eq!(err.code, ErrorCode::ScanFailed);
        assert!(err.message.contains("frame 4"), "{}", err.message);
    }

    #[test]
    fn an_error_from_on_frame_stops_the_scan() {
        let roll = Roll::new("onframe");
        let mut seen = Vec::new();
        let result = roll.source().scan(
            &params(1..=3),
            &mut |n, _| {
                seen.push(n);
                if n == 2 {
                    Err(AppError::new(ErrorCode::Io, "disk full"))
                } else {
                    Ok(())
                }
            },
            &|_| {},
            &CancelToken::default(),
        );
        assert_eq!(seen, [1, 2], "nothing after the failing frame");
        assert_eq!(result.unwrap_err().code, ErrorCode::Io);
    }

    #[test]
    fn a_cancel_before_a_frame_stops_with_cancelled_and_keeps_the_frames_done() {
        let roll = Roll::new("cancel");
        let before = CancelToken::default();
        before.cancel();
        let out = run(&roll.source(), &params(1..=3), &before);
        assert!(out.frames.is_empty());
        assert_eq!(out.result.unwrap_err().code, ErrorCode::Cancelled);

        let token = CancelToken::default();
        let mid = token.clone();
        let mut seen = Vec::new();
        let result = roll.source().scan(
            &params(1..=3),
            &mut |n, _| {
                seen.push(n);
                mid.cancel();
                Ok(())
            },
            &|_| {},
            &token,
        );
        assert_eq!(
            seen,
            [1],
            "frame 1 was handed over, then the cancel was seen"
        );
        assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    }

    #[test]
    fn bad_parameters_and_other_devices_are_refused_before_anything_is_read() {
        let roll = Roll::new("refuse");
        let source = roll.source();
        let invalid = ScanParams {
            dpi: 3000,
            ..params(1..=3)
        };
        let out = run(&source, &invalid, &CancelToken::default());
        assert_eq!(out.result.unwrap_err().code, ErrorCode::InvalidInput);
        let other = ScanParams::new("ls50", 1..=3);
        let out = run(&source, &other, &CancelToken::default());
        assert_eq!(out.result.unwrap_err().code, ErrorCode::ScannerUnavailable);
        assert!(out.frames.is_empty() && out.progress.is_empty());
    }

    #[test]
    fn a_missing_folder_means_no_scanner_and_a_listing_names_the_frames() {
        let roll = Roll::new("list");
        let devices = roll.source().list_devices().unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, FIXTURE_DEVICE_ID);
        assert!(devices[0].name.contains("3 frames"), "{}", devices[0].name);

        let missing = FixtureSource::new(roll.dir.join("nothing"));
        assert_eq!(
            missing.list_devices().unwrap_err().code,
            ErrorCode::ScannerUnavailable
        );
        let out = run(&missing, &params(1..=1), &CancelToken::default());
        assert_eq!(out.result.unwrap_err().code, ErrorCode::ScannerUnavailable);
    }

    #[test]
    fn a_file_that_is_not_a_colour_scan_fails_that_frame() {
        let roll = Roll::new("gray");
        let gray = Tiff16 {
            width: 4,
            height: 4,
            channels: 1,
            data: vec![1; 16],
        };
        write_tiff16(&roll.dir.join("0004.tif"), &gray, Compression::None).unwrap();
        let out = run(&roll.source(), &params(1..=4), &CancelToken::default());
        assert_eq!(numbers(&out), [1, 2, 3]);
        assert_eq!(out.result.unwrap_err().code, ErrorCode::ScanFailed);
    }

    /// Replays the first scans of the private samples folder. Run by hand with
    /// `cargo test -p gtp-ingest -- --ignored`; skips when the samples are missing.
    #[test]
    #[ignore = "reads the private samples"]
    fn the_private_scans_replay() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/private");
        if !dir.is_dir() {
            eprintln!("no private samples; skipped");
            return;
        }
        let source = FixtureSource::new(&dir);
        let devices = source.list_devices().unwrap();
        eprintln!("{}", devices[0].name);
        let out = run(&source, &params(1..=2), &CancelToken::default());
        assert!(out.result.is_ok(), "{:?}", out.result);
        assert_eq!(numbers(&out), [1, 2]);
        assert!(out
            .frames
            .iter()
            .all(|(_, s)| s.width > 1000 && s.height > 1000));
    }
}
