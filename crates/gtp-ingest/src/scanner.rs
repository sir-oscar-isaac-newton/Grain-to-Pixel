//! The scanner port (specification section 6, "Scanner abstraction"): what the roll-scan job needs
//! from a film scanner, with validated parameters, so it runs the same against the Nikon Coolscan
//! LS-50 (`NkscanSource`, later) and against [`crate::FixtureSource`] in tests and development.
//!
//! Roll scans take a long time: every finished frame is handed to `on_frame` at once, so a cancel
//! or an error keeps the frames done so far and the scan can resume from the next frame number.

use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtp_core::{AppError, ErrorCode};

/// The most frames a roll can hold.
pub const MAX_FRAMES: u32 = 40;
/// The resolutions a scan may ask for.
pub const SUPPORTED_DPI: [u32; 3] = [1000, 2000, 4000];
/// The resolution when none is chosen.
pub const DEFAULT_DPI: u32 = 4000;
/// How many reads of each pixel the scanner may average.
pub const SAMPLE_COUNTS: [u8; 5] = [1, 2, 4, 8, 16];

/// A scanner the app can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
}

/// What to scan. Build it with [`ScanParams::new`] and check it with [`ScanParams::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanParams {
    pub device_id: String,
    /// First to last frame of the roll, counted from 1.
    pub frames: RangeInclusive<u32>,
    pub dpi: u32,
    /// Reads averaged per pixel (multi-sampling).
    pub samples: u8,
    /// Also scan the infrared plane.
    pub ir: bool,
    pub autofocus: bool,
}

impl ScanParams {
    /// Parameters for `frames` on `device_id` with the usual settings: 4000 dpi, one sample, with
    /// infrared, no autofocus.
    pub fn new(device_id: impl Into<String>, frames: RangeInclusive<u32>) -> Self {
        Self {
            device_id: device_id.into(),
            frames,
            dpi: DEFAULT_DPI,
            samples: 1,
            ir: true,
            autofocus: false,
        }
    }

    /// How many frames this scan covers.
    pub fn frame_count(&self) -> u32 {
        (self.frames.end() + 1).saturating_sub(*self.frames.start())
    }

    /// `InvalidInput` for an empty device id, frames outside 1..=40 (or last before first), a
    /// resolution that is not supported, or a sample count that is not 1, 2, 4, 8 or 16.
    pub fn validate(&self) -> Result<(), AppError> {
        let invalid = |message: &str, details: String| {
            Err(AppError::new(ErrorCode::InvalidInput, message).with_details(details))
        };
        if self.device_id.trim().is_empty() {
            return invalid("Choose a scanner.", "empty device id".into());
        }
        let (first, last) = (*self.frames.start(), *self.frames.end());
        if first < 1 || last > MAX_FRAMES || first > last {
            return invalid(
                "Choose frames from 1 to 40, first before last.",
                format!("frames {first}..={last}"),
            );
        }
        if !SUPPORTED_DPI.contains(&self.dpi) {
            return invalid(
                "That resolution is not supported; choose 1000, 2000 or 4000 dpi.",
                format!("{} dpi", self.dpi),
            );
        }
        if !SAMPLE_COUNTS.contains(&self.samples) {
            return invalid(
                "Samples per pixel must be 1, 2, 4, 8 or 16.",
                format!("{} samples", self.samples),
            );
        }
        Ok(())
    }
}

/// How a frame was scanned, kept with its originals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanMeta {
    pub device_id: String,
    pub samples: u8,
    pub autofocus: bool,
}

/// One scanned frame: 16-bit samples, row-major and interleaved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawScan {
    pub width: u32,
    pub height: u32,
    /// `width * height * 3` samples.
    pub rgb16: Vec<u16>,
    /// `width * height` samples, when the scan has an infrared plane.
    pub ir16: Option<Vec<u16>>,
    pub dpi: u32,
    pub meta: ScanMeta,
}

/// Asks a running scan to stop. Cheap to clone; every clone sees the same flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// `Err(Cancelled)` once cancelled, for use with `?` between steps.
    pub fn check(&self) -> Result<(), AppError> {
        if self.is_cancelled() {
            Err(AppError::new(
                ErrorCode::Cancelled,
                "The scan was cancelled.",
            ))
        } else {
            Ok(())
        }
    }
}

/// A film scanner.
pub trait ScannerSource: Send + Sync {
    /// The scanners that can be used right now; `ScannerUnavailable` when none can be reached.
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AppError>;

    /// Scans `p.frames` in order, calling `on_frame(frame_number, scan)` as each one completes so
    /// it can be stored at once. `progress` gets 0 to 1 over the whole scan. Stops with
    /// `Cancelled` once `cancel` is set, and with the error of `on_frame` if that fails; frames
    /// already handed over stay handed over.
    fn scan(
        &self,
        p: &ScanParams,
        on_frame: &mut dyn FnMut(u32, RawScan) -> Result<(), AppError>,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<(), AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> ScanParams {
        ScanParams::new("ls50", 1..=36)
    }

    fn code(p: &ScanParams) -> Option<ErrorCode> {
        p.validate().err().map(|e| e.code)
    }

    #[test]
    fn the_usual_parameters_are_valid() {
        let p = params();
        assert_eq!(p.validate(), Ok(()));
        assert_eq!(
            (p.dpi, p.samples, p.ir, p.autofocus),
            (4000, 1, true, false)
        );
        assert_eq!(p.frame_count(), 36);
        assert_eq!(ScanParams::new("x", 1..=40).validate(), Ok(()));
        assert_eq!(ScanParams::new("x", 7..=7).frame_count(), 1);
    }

    // Reversed ranges are the point of this test.
    #[allow(clippy::reversed_empty_ranges)]
    #[test]
    fn frames_outside_1_to_40_or_in_the_wrong_order_are_rejected() {
        for frames in [0..=5, 1..=41, 41..=41, 0..=0, 5..=4, 40..=1] {
            let p = ScanParams::new("ls50", frames.clone());
            assert_eq!(code(&p), Some(ErrorCode::InvalidInput), "{frames:?}");
        }
        assert_eq!(ScanParams::new("ls50", 40..=40).validate(), Ok(()));
        assert_eq!(ScanParams::new("ls50", 1..=1).validate(), Ok(()));
    }

    #[test]
    fn only_1000_2000_and_4000_dpi_are_supported() {
        for dpi in [0, 500, 999, 3999, 4001, 8000] {
            let p = ScanParams { dpi, ..params() };
            assert_eq!(code(&p), Some(ErrorCode::InvalidInput), "{dpi}");
        }
        assert_eq!(
            ScanParams {
                dpi: 4000,
                ..params()
            }
            .validate(),
            Ok(())
        );
    }

    #[test]
    fn sample_counts_are_1_2_4_8_or_16() {
        for samples in [1, 2, 4, 8, 16] {
            assert_eq!(
                ScanParams {
                    samples,
                    ..params()
                }
                .validate(),
                Ok(()),
                "{samples}"
            );
        }
        for samples in [0, 3, 5, 6, 7, 9, 15, 17, 32, 255] {
            let p = ScanParams {
                samples,
                ..params()
            };
            assert_eq!(code(&p), Some(ErrorCode::InvalidInput), "{samples}");
        }
    }

    #[test]
    fn a_scan_needs_a_device() {
        assert_eq!(
            code(&ScanParams::new("", 1..=3)),
            Some(ErrorCode::InvalidInput)
        );
        assert_eq!(
            code(&ScanParams::new("  ", 1..=3)),
            Some(ErrorCode::InvalidInput)
        );
    }

    #[test]
    fn a_rejected_value_is_named_in_the_details() {
        let err = ScanParams {
            dpi: 300,
            ..params()
        }
        .validate()
        .unwrap_err();
        assert_eq!(err.details.as_deref(), Some("300 dpi"));
    }

    #[test]
    fn cancelling_one_clone_cancels_them_all() {
        let token = CancelToken::default();
        let clone = token.clone();
        assert!(!clone.is_cancelled());
        assert_eq!(clone.check(), Ok(()));
        token.cancel();
        assert!(clone.is_cancelled());
        assert_eq!(clone.check().unwrap_err().code, ErrorCode::Cancelled);
    }
}
