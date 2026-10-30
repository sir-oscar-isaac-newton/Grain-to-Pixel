//! The in-memory image every pipeline stage works on: linear f32, row-major, interleaved channels.

use crate::error::{AppError, ErrorCode};

/// Linear f32 pixels, row-major, `channels` interleaved values per pixel (1 to 4).
#[derive(Debug, Clone, PartialEq)]
pub struct ImageF32 {
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub data: Vec<f32>,
}

impl ImageF32 {
    /// Wraps existing pixel data, checking that its length matches the shape.
    pub fn new(width: u32, height: u32, channels: u8, data: Vec<f32>) -> Result<Self, AppError> {
        let expected = Self::sample_count(width, height, channels)?;
        if data.len() != expected {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "The image data does not match its size.",
            )
            .with_details(format!(
                "{width}x{height}x{channels} needs {expected} samples, got {}",
                data.len()
            )));
        }
        Ok(Self {
            width,
            height,
            channels,
            data,
        })
    }

    /// An all-zero image of the given shape.
    pub fn zeros(width: u32, height: u32, channels: u8) -> Result<Self, AppError> {
        let count = Self::sample_count(width, height, channels)?;
        Self::new(width, height, channels, vec![0.0; count])
    }

    fn sample_count(width: u32, height: u32, channels: u8) -> Result<usize, AppError> {
        if width == 0 || height == 0 || !(1..=4).contains(&channels) {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "An image needs a non-zero size and 1 to 4 channels.",
            )
            .with_details(format!("{width}x{height}x{channels}")));
        }
        usize::try_from(width)
            .ok()
            .and_then(|w| w.checked_mul(usize::try_from(height).ok()?))
            .and_then(|n| n.checked_mul(usize::from(channels)))
            .ok_or_else(|| {
                AppError::new(ErrorCode::InvalidInput, "The image is too large.")
                    .with_details(format!("{width}x{height}x{channels}"))
            })
    }

    fn row_len(&self) -> usize {
        self.width as usize * usize::from(self.channels)
    }

    /// The samples of row `y`, or `None` past the bottom edge.
    pub fn row(&self, y: u32) -> Option<&[f32]> {
        let len = self.row_len();
        let start = (y as usize).checked_mul(len)?;
        self.data.get(start..start.checked_add(len)?)
    }

    /// Mutable access to row `y`, or `None` past the bottom edge.
    pub fn row_mut(&mut self, y: u32) -> Option<&mut [f32]> {
        let len = self.row_len();
        let start = (y as usize).checked_mul(len)?;
        self.data.get_mut(start..start.checked_add(len)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_checks_the_data_length() {
        assert!(ImageF32::new(2, 2, 3, vec![0.0; 12]).is_ok());
        let err = ImageF32::new(2, 2, 3, vec![0.0; 11]).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn empty_shapes_and_bad_channel_counts_are_rejected() {
        for (w, h, c) in [(0, 1, 1), (1, 0, 1), (1, 1, 0), (1, 1, 5)] {
            assert!(ImageF32::zeros(w, h, c).is_err(), "{w}x{h}x{c}");
        }
    }

    #[test]
    fn an_oversized_shape_is_an_error_not_an_overflow() {
        let err = ImageF32::zeros(u32::MAX, u32::MAX, 4).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn rows_are_addressable_and_bounded() {
        let mut image = ImageF32::zeros(3, 2, 2).unwrap();
        image.row_mut(1).unwrap().fill(7.0);
        assert_eq!(image.row(0).unwrap(), [0.0; 6]);
        assert_eq!(image.row(1).unwrap(), [7.0; 6]);
        assert!(image.row(2).is_none());
        assert!(image.row_mut(u32::MAX).is_none());
    }
}
