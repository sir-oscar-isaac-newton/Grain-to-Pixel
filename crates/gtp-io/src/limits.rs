//! Size limits for untrusted images, checked from the header before any pixel buffer exists.

use gtp_core::{AppError, ErrorCode};

/// Largest accepted image (docs/specification.md section 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum width or height in pixels.
    pub max_side: u32,
    /// Maximum size of the decoded samples in bytes.
    pub max_decoded_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_side: 20_000,
            max_decoded_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

impl Limits {
    /// Checks a declared image shape and returns its sample count (`width * height * channels`).
    /// Uses 64-bit arithmetic that cannot overflow for sides within `max_side`.
    pub fn check(
        &self,
        width: u32,
        height: u32,
        channels: u8,
        bytes_per_sample: u8,
    ) -> Result<usize, AppError> {
        let too_large = |what: &str| {
            AppError::new(
                ErrorCode::InvalidInput,
                "This image is larger than the app accepts.",
            )
            .with_details(format!(
                "{what}: {width}x{height}x{channels} (limits: {} px per side, {} bytes)",
                self.max_side, self.max_decoded_bytes
            ))
        };
        if width == 0 || height == 0 || channels == 0 {
            return Err(AppError::new(
                ErrorCode::UnsupportedFormat,
                "This image has no pixels.",
            ));
        }
        if width > self.max_side || height > self.max_side {
            return Err(too_large("side too long"));
        }
        let samples = u64::from(width) * u64::from(height) * u64::from(channels);
        let bytes = samples.saturating_mul(u64::from(bytes_per_sample));
        if bytes > self.max_decoded_bytes {
            return Err(too_large("too many bytes"));
        }
        usize::try_from(samples).map_err(|_| too_large("too many samples"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_specified_limits() {
        let limits = Limits::default();
        assert_eq!(limits.max_side, 20_000);
        assert_eq!(limits.max_decoded_bytes, 2_147_483_648);
    }

    #[test]
    fn a_shape_within_the_limits_returns_its_sample_count() {
        assert_eq!(
            Limits::default().check(6016, 4016, 3, 2).unwrap(),
            6016 * 4016 * 3
        );
    }

    #[test]
    fn huge_shapes_are_rejected_without_overflow() {
        let limits = Limits::default();
        for (w, h, c) in [
            (100_000, 100_000, 3),
            (u32::MAX, u32::MAX, 4),
            (20_001, 1, 1),
            (1, 20_001, 1),
        ] {
            let err = limits.check(w, h, c, 2).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{w}x{h}x{c}");
        }
        // 20000 x 20000 x 4 x 2 bytes = 3.2 GB is within the side limit but over the byte limit.
        assert_eq!(
            limits.check(20_000, 20_000, 4, 2).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn empty_shapes_are_unsupported() {
        assert_eq!(
            Limits::default().check(0, 5, 1, 2).unwrap_err().code,
            ErrorCode::UnsupportedFormat
        );
    }
}
