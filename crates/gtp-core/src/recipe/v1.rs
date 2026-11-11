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
    pub mono: Mono,
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
            mono: Mono::default(),
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
    /// Saturation that spares colours which are already vivid; -100 to 100.
    pub vibrance: f32,
    /// Tints the highlights and the shadows (a stage of its own after the tone).
    pub split_tone: SplitTone,
    /// Per-hue changes of hue, saturation and lightness (a stage of its own after the colour).
    #[serde(default)]
    pub mixer: Mixer,
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
            vibrance: 0.0,
            split_tone: SplitTone::default(),
            mixer: Mixer::default(),
        }
    }
}

/// Number of hue bands of the colour mixer.
pub const MIXER_BANDS: usize = 8;

/// The colour mixer: per hue band, a shift of hue, a change of saturation and a change of
/// lightness (docs/specification.md section 4.2). Every value runs from -100 to 100 and 0 changes
/// nothing. The bands, in index order 0 to 7, are fixed: red, orange, yellow, green, aqua, blue,
/// purple, magenta. A recipe saved before the mixer existed loads with zeros; a list of another
/// length than 8 is a malformed recipe.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Mixer {
    /// -100 to 100 per band: shifts the hue, +100 by 30 degrees towards the next band.
    pub hue: [f32; MIXER_BANDS],
    /// -100 to 100 per band: -100 removes the colour, +100 doubles it.
    pub saturation: [f32; MIXER_BANDS],
    /// -100 to 100 per band: -100 darkens by one stop, +100 lightens by one stop.
    pub lightness: [f32; MIXER_BANDS],
}

/// Split toning: one tint for the highlights and one for the shadows, blended in at the same
/// luma so brightness never changes (docs/specification.md section 4.2). A recipe saved before it
/// existed loads with the defaults, which change nothing (both strengths are 0).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct SplitTone {
    /// 0 to 360 degrees: the hue of the highlight tint.
    pub highlight_hue: f32,
    /// 0 to 100: how much of the highlight tint is blended in; 0 is off.
    pub highlight_strength: f32,
    /// 0 to 360 degrees: the hue of the shadow tint.
    pub shadow_hue: f32,
    /// 0 to 100: how much of the shadow tint is blended in; 0 is off.
    pub shadow_strength: f32,
    /// -100 to 100: moves the crossover; +100 gives the highlight tint to every tone, -100 the
    /// shadow tint, 0 crosses at the middle.
    pub balance: f32,
}

impl Default for SplitTone {
    fn default() -> Self {
        Self {
            highlight_hue: 40.0,
            highlight_strength: 0.0,
            shadow_hue: 220.0,
            shadow_strength: 0.0,
            balance: 0.0,
        }
    }
}

/// Rec.2020 luma weights: the default black-and-white mix.
pub const REC2020_LUMA: [f32; 3] = [0.2627, 0.6780, 0.0593];

/// Black-and-white conversion. When enabled, every pixel becomes `mix[0]*R + mix[1]*G + mix[2]*B`
/// in all three channels, which acts like a coloured filter on black-and-white film. Weights run
/// from -2 to 2; weights that sum to 1 keep the brightness.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Mono {
    pub enabled: bool,
    pub mix: [f32; 3],
}

impl Default for Mono {
    fn default() -> Self {
        Self {
            enabled: false,
            mix: REC2020_LUMA,
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
/// blacks, dehaze, then the curves; the vignette is a stage of its own after the geometry. The sliders from `contrast` on run from -100 to 100.
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
    /// Removes (positive) or adds (negative) a uniform veil such as film fog or scanner flare.
    pub dehaze: f32,
    pub curve: Curves,
    /// Darkens or lightens the corners (the last stage, after the geometry).
    pub vignette: Vignette,
}

/// Vignette: a gain on the distance from the centre of the final picture (docs/specification.md
/// section 4.2). A recipe saved before it existed loads with the defaults, which change nothing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Vignette {
    /// -100 to 100: negative darkens the corners, positive lightens them, 0 changes nothing.
    pub amount: f32,
    /// 0 to 100: how far in towards the centre the effect reaches; lower reaches further in.
    pub midpoint: f32,
}

impl Default for Vignette {
    fn default() -> Self {
        Self {
            amount: 0.0,
            midpoint: 50.0,
        }
    }
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
            dehaze: 0.0,
            curve: Curves::default(),
            vignette: Vignette::default(),
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

/// The radius of the output sharpening when none is chosen, in pixels of the exported image.
pub const DEFAULT_SHARPEN_RADIUS_PX: f32 = 1.0;

/// Output sharpening: an unsharp mask on the luminance of the exported image. It is applied once,
/// at export, to the image at its final size, never to the working image or the preview.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Sharpen {
    /// How much of the difference between the image and its blur is added back, in percent;
    /// 0 changes nothing. The limit is 150.
    pub amount: f32,
    /// The blur's radius (the standard deviation of the Gaussian) in pixels of the exported image,
    /// from 0.3 to 3. It is not scaled to the image: a larger export wants a larger radius for the
    /// same look.
    pub radius_px: f32,
}

impl Default for Sharpen {
    fn default() -> Self {
        Self {
            amount: 0.0,
            radius_px: DEFAULT_SHARPEN_RADIUS_PX,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Output {
    pub space: OutputSpace,
    pub sharpen: Sharpen,
}
