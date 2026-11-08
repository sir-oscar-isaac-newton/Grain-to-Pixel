//! How a picture was exposed, as far as the camera's EXIF says (H-15): the shutter time, the ISO
//! and the aperture. Saved with scanlight levels, so the app can warn when a capture was made with
//! other camera settings than the levels were balanced for.

use serde::{Deserialize, Serialize};

/// A fraction as the camera wrote it (an exposure of 1/60 s is 1 over 60).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ExposureRatio {
    pub num: u32,
    pub den: u32,
}

impl ExposureRatio {
    /// True when both are the same fraction (1/60 and 2/120 are).
    pub fn same_as(self, other: ExposureRatio) -> bool {
        u64::from(self.num) * u64::from(other.den) == u64::from(other.num) * u64::from(self.den)
    }
}

/// The shutter time, ISO and aperture of an exposure; each is `None` when the EXIF has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct ExposureInfo {
    /// The shutter time in seconds.
    pub exposure_time: Option<ExposureRatio>,
    pub iso: Option<u32>,
    /// The aperture, as an f-number.
    pub f_number: Option<ExposureRatio>,
}

impl ExposureInfo {
    /// True when no field is known.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// True when a field that both have differs. A field only one side knows is no difference.
    pub fn differs_from(&self, other: &ExposureInfo) -> bool {
        let ratio = |a: Option<ExposureRatio>, b: Option<ExposureRatio>| match (a, b) {
            (Some(a), Some(b)) => !a.same_as(b),
            _ => false,
        };
        ratio(self.exposure_time, other.exposure_time)
            || ratio(self.f_number, other.f_number)
            || matches!((self.iso, other.iso), (Some(a), Some(b)) if a != b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(time: (u32, u32), iso: u32, f: (u32, u32)) -> ExposureInfo {
        ExposureInfo {
            exposure_time: Some(ExposureRatio {
                num: time.0,
                den: time.1,
            }),
            iso: Some(iso),
            f_number: Some(ExposureRatio { num: f.0, den: f.1 }),
        }
    }

    #[test]
    fn equal_settings_do_not_differ_even_as_other_fractions() {
        assert!(!info((1, 60), 200, (56, 10)).differs_from(&info((2, 120), 200, (28, 5))));
    }

    #[test]
    fn each_field_that_both_sides_know_can_differ() {
        let base = info((1, 60), 200, (56, 10));
        assert!(base.differs_from(&info((1, 125), 200, (56, 10))));
        assert!(base.differs_from(&info((1, 60), 400, (56, 10))));
        assert!(base.differs_from(&info((1, 60), 200, (8, 1))));
    }

    #[test]
    fn a_field_only_one_side_knows_is_no_difference() {
        let base = info((1, 60), 200, (56, 10));
        assert!(!base.differs_from(&ExposureInfo::default()));
        assert!(!ExposureInfo::default().differs_from(&base));
        let only_iso = ExposureInfo {
            iso: Some(200),
            ..ExposureInfo::default()
        };
        assert!(!base.differs_from(&only_iso));
        assert!(ExposureInfo::default().is_empty());
    }
}
