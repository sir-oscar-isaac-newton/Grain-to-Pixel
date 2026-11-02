//! RAW input for RGB triplets (docs/specification.md section 4.2 step 1): a camera file decodes to
//! the linear sensor samples of its Bayer mosaic ([`RawLinear`]), which a bilinear demosaic turns
//! into a plane or a picture. No white balance, no tone curve, no colour matrix: the three
//! exposures of a triplet are lit by one LED colour each, and only the lit channel of each is used.
//!
//! The decoder is behind the [`RawDecoder`] trait so merges can be tested without camera files.
//! [`RawlerDecoder`] is the real one (decision: `decisions/0007-nef-decoder.md`).

mod demosaic;
mod nef;

use gtp_core::{AppError, ErrorCode};

pub use demosaic::{demosaic_bilinear, demosaic_channel};
pub use nef::{decode_nef, RawDecoder, RawlerDecoder, MAX_RAW_FILE_BYTES};

/// The colour of each photosite of a 2 x 2 block of the mosaic, as 0 = red, 1 = green, 2 = blue, in
/// reading order: (0,0), (1,0), (0,1), (1,1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cfa(pub [u8; 4]);

impl Cfa {
    pub const RGGB: Cfa = Cfa([0, 1, 1, 2]);
    pub const BGGR: Cfa = Cfa([2, 1, 1, 0]);
    pub const GRBG: Cfa = Cfa([1, 0, 2, 1]);
    pub const GBRG: Cfa = Cfa([1, 2, 0, 1]);

    /// The colour (0 red, 1 green, 2 blue) of the photosite at column `x`, row `y`.
    pub fn color_at(self, x: usize, y: usize) -> usize {
        usize::from(self.0[((y & 1) << 1) | (x & 1)])
    }

    /// True for a layout with one red, two green and one blue photosite per block.
    fn is_bayer(self) -> bool {
        let count = |c: u8| self.0.iter().filter(|v| **v == c).count();
        self.0.iter().all(|v| *v <= 2) && count(0) == 1 && count(1) == 2 && count(2) == 1
    }
}

/// The sensor samples of one exposure, cropped to the camera's image area: 16-bit, one per
/// photosite, linear in the light that fell on it.
#[derive(Debug, Clone, PartialEq)]
pub struct RawLinear {
    pub width: u32,
    pub height: u32,
    pub cfa: Cfa,
    /// The black level of each photosite of a 2 x 2 block, in the order of [`Cfa`]. D600 files have
    /// it subtracted already (0).
    pub black: [f32; 4],
    /// The sample value of full scale.
    pub white: f32,
    /// `width * height` samples, row-major.
    pub data: Vec<u16>,
}

impl RawLinear {
    /// Checks the shape: a Bayer layout, a mosaic of at least 2 x 2, data that fits it and a white
    /// level above every black level.
    pub fn validate(&self) -> Result<(), AppError> {
        let invalid = |message: &str, details: String| {
            Err(AppError::new(ErrorCode::InvalidInput, message).with_details(details))
        };
        if !self.cfa.is_bayer() {
            return invalid(
                "The sensor is not a Bayer mosaic.",
                format!("{:?}", self.cfa),
            );
        }
        if self.width < 2 || self.height < 2 {
            return invalid(
                "The raw image is too small.",
                format!("{}x{}", self.width, self.height),
            );
        }
        let expected = u64::from(self.width) * u64::from(self.height);
        if self.data.len() as u64 != expected {
            return invalid(
                "The raw data does not match its size.",
                format!(
                    "{}x{} needs {expected} samples, got {}",
                    self.width,
                    self.height,
                    self.data.len()
                ),
            );
        }
        if self
            .black
            .iter()
            .any(|b| !b.is_finite() || *b < 0.0 || *b >= self.white)
            || !self.white.is_finite()
        {
            return invalid(
                "The raw image has no usable range between black and white.",
                format!("black {:?}, white {}", self.black, self.white),
            );
        }
        Ok(())
    }
}
