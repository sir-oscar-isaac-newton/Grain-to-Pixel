//! Recipe version 1. Unknown fields are ignored and missing fields take their defaults, so older
//! and newer files of the same version both load. Values are brought into range by `Recipe::clamp`.

use serde::{Deserialize, Serialize};

use super::migrate::CURRENT_VERSION;

/// One point of a tone curve: `[input, output]`, both in 0 to 1.
pub type CurvePoint = [f32; 2];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Recipe {
    pub v: u32,
    pub geometry: Geometry,
    pub base: Base,
    pub invert: Invert,
    pub color: Color,
    pub tone: Tone,
    pub dust: Dust,
    pub output: Output,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            v: CURRENT_VERSION,
            geometry: Geometry::default(),
            base: Base::default(),
            invert: Invert::default(),
            color: Color::default(),
            tone: Tone::default(),
            dust: Dust::default(),
            output: Output::default(),
        }
    }
}

/// Crop rectangle as fractions of the full frame (0 to 1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Default for Crop {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Geometry {
    pub crop: Crop,
    /// Straighten angle in degrees.
    pub angle_deg: f32,
    /// Quarter turns clockwise; normalized to 0 to 3.
    pub rot90: i32,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            crop: Crop::default(),
            angle_deg: 0.0,
            rot90: 0,
            flip_h: false,
            flip_v: false,
        }
    }
}

/// Where the film base comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum BaseSource {
    /// The roll's measured film base.
    #[default]
    Roll,
    /// Sampled by the user for this frame.
    Manual,
}

/// Film base in linear scan values; every channel is divided by it before inversion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Base {
    pub source: BaseSource,
    pub rgb: [f32; 3],
}

impl Default for Base {
    fn default() -> Self {
        Self {
            source: BaseSource::Roll,
            rgb: [1.0; 3],
        }
    }
}

/// Per-channel density range mapped to the positive image (auto-filled from the histogram).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Invert {
    pub black: [f32; 3],
    pub white: [f32; 3],
}

impl Default for Invert {
    fn default() -> Self {
        Self {
            black: [0.0; 3],
            white: [1.0; 3],
        }
    }
}

/// Colour sliders, all from -100 to 100 except saturation. Three opposing pairs, as in darkroom
/// filtration: a positive `red_cyan` adds red, a negative one adds cyan, and so on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Color {
    pub red_cyan: f32,
    pub green_magenta: f32,
    pub blue_yellow: f32,
    pub temperature: f32,
    pub tint: f32,
    /// 0 is monochrome, 1 unchanged, 2 doubled.
    pub saturation: f32,
}

impl Default for Color {
    fn default() -> Self {
        Self {
            red_cyan: 0.0,
            green_magenta: 0.0,
            blue_yellow: 0.0,
            temperature: 0.0,
            tint: 0.0,
            saturation: 1.0,
        }
    }
}

/// Tone curves; an empty list means "no change" for that curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Curves {
    pub master: Vec<CurvePoint>,
    pub r: Vec<CurvePoint>,
    pub g: Vec<CurvePoint>,
    pub b: Vec<CurvePoint>,
}

impl Default for Curves {
    fn default() -> Self {
        Self {
            master: vec![[0.0, 0.0], [1.0, 1.0]],
            r: Vec::new(),
            g: Vec::new(),
            b: Vec::new(),
        }
    }
}

/// Tone sliders. Applied in this order: exposure, contrast, highlights and shadows, whites and
/// blacks, then the curves. The sliders from `contrast` on run from -100 to 100.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Tone {
    pub exposure_ev: f32,
    pub contrast: f32,
    /// Brightens or darkens only the bright part of the tonal range.
    pub highlights: f32,
    /// Brightens or darkens only the dark part of the tonal range.
    pub shadows: f32,
    /// Moves the point that counts as pure white.
    pub whites: f32,
    /// Moves the point that counts as pure black.
    pub blacks: f32,
    pub curve: Curves,
}

impl Default for Tone {
    fn default() -> Self {
        Self {
            exposure_ev: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            curve: Curves::default(),
        }
    }
}

/// Infrared dust removal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Dust {
    pub enabled: bool,
    /// Threshold on the normalized IR plane, 0 to 1.
    pub threshold: f32,
    /// How far the mask is grown, in pixels.
    pub grow_px: u32,
}

impl Default for Dust {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: 0.5,
            grow_px: 2,
        }
    }
}

/// Export colour space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum OutputSpace {
    #[default]
    Srgb,
    AdobeRgb,
    Prophoto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Output {
    pub space: OutputSpace,
}
