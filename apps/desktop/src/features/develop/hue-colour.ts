/**
 * Screen colours for the hue and band sliders of the Colour panel. They are decoration and a guide
 * for the screen in sRGB, not a proof of what the pipeline makes of an image.
 */
import type { ColourKey } from './recipe-parts';

/** Degrees in a full turn of the hue. */
const FULL_TURN_DEG = 360;
/** Degrees in one of the six sectors of the colour wheel (as in `split_tone.rs`). */
const SECTOR_DEG = 60;
const SECTORS = 6;
/** Where each channel's hue ramp is offset on the wheel (red, green, blue), in sectors. */
const HUE_OFFSETS = [5, 3, 1] as const;
const RAMP_CENTRE = 2;
const RAMP_FLOOR = 1;
/** Degrees between two stops of the hue gradient. */
const GRADIENT_STEP_DEG = 30;
const CHANNEL_MAX = 255;

/**
 * Linear Rec.2020 to linear sRGB (Rec.709 primaries): the inverse of the matrix of Recommendation
 * ITU-R BT.2087 (colour conversion from Rec.709 to Rec.2020), as printed in BT.2407.
 */
const REC2020_TO_SRGB = [
  [1.660491, -0.587641, -0.07285],
  [-0.12455, 1.1329, -0.008349],
  [-0.018151, -0.100579, 1.11873],
] as const;

/**
 * The centres of the eight bands of the colour mixer, in degrees of Oklch hue. They must match
 * `MIXER_BAND_HUES` in `crates/gtp-edit/src/mixer.rs` (ADR 0012); a test compares the two.
 */
export const MIXER_BAND_HUES: readonly number[] = [29, 70, 110, 143, 195, 264, 296, 328];

/** Lightness and chroma of a band swatch, the same for all bands. */
const BAND_LIGHTNESS = 0.72;
const BAND_CHROMA = 0.15;

/** The fully saturated colour of a hue (HSV, value and saturation 1): the ramp of `hue_rgb`. */
function hueRgb(hueDeg: number): [number, number, number] {
  const sector = (((hueDeg % FULL_TURN_DEG) + FULL_TURN_DEG) % FULL_TURN_DEG) / SECTOR_DEG;
  const channel = (offset: number) => {
    const distance = Math.abs(((sector + offset) % SECTORS) - RAMP_CENTRE);
    return Math.min(1, Math.max(0, distance - RAMP_FLOOR));
  };
  return [channel(HUE_OFFSETS[0]), channel(HUE_OFFSETS[1]), channel(HUE_OFFSETS[2])];
}

const encode = (linear: number) =>
  linear <= 0.0031308 ? 12.92 * linear : 1.055 * linear ** (1 / 2.4) - 0.055;

/** A linear Rec.2020 triple as linear sRGB, each channel clipped to [0, 1]. */
function clippedSrgb(rgb2020: readonly [number, number, number]): number[] {
  return REC2020_TO_SRGB.map((row) =>
    Math.min(1, Math.max(0, row[0] * rgb2020[0] + row[1] * rgb2020[1] + row[2] * rgb2020[2])),
  );
}

/**
 * The screen colour of a split toning hue: the hue's fully saturated colour (read as linear
 * Rec.2020, as the pipeline does) in sRGB, clipped to the gamut and shown at full brightness.
 * The hue wraps at 360 degrees.
 */
export function tintCss(hueDeg: number): string {
  const linear = clippedSrgb(hueRgb(hueDeg));
  const peak = Math.max(...linear, Number.EPSILON);
  const [r, g, b] = linear.map((c) => Math.round(encode(c / peak) * CHANNEL_MAX));
  return `rgb(${r} ${g} ${b})`;
}

/** A left to right gradient over the whole hue range, a stop every 30 degrees (13 stops). */
export function hueGradientCss(): string {
  const stops = Array.from({ length: FULL_TURN_DEG / GRADIENT_STEP_DEG + 1 }, (_, i) =>
    tintCss(i * GRADIENT_STEP_DEG),
  );
  return `linear-gradient(to right, ${stops.join(', ')})`;
}

/**
 * The largest turn of the hue by a band's Hue slider, in degrees (at -100 or +100). It must match
 * `MIXER_HUE_MAX_DEG` in `crates/gtp-edit/src/mixer.rs`; a test compares the two.
 */
export const MIXER_HUE_MAX_DEG = 30;

/** A colour of the band swatches and gradients, as CSS `oklch()`; the hue wraps into [0, 360). */
function bandColourCss(hueDeg: number): string {
  const hue = ((hueDeg % FULL_TURN_DEG) + FULL_TURN_DEG) % FULL_TURN_DEG;
  return `oklch(${BAND_LIGHTNESS} ${BAND_CHROMA} ${hue}deg)`;
}

/** The colour of a mixer band (its index in the recipe), as CSS `oklch()`. */
export function bandCss(index: number): string {
  return bandColourCss(MIXER_BAND_HUES[index] ?? 0);
}

/**
 * A left to right gradient for the Hue slider of a band: the colour it turns towards at -100, the
 * band's own at 0 and the colour at +100, by the largest turn of the pipeline.
 */
export function bandHueGradientCss(index: number): string {
  const hue = MIXER_BAND_HUES[index] ?? 0;
  const stops = [hue - MIXER_HUE_MAX_DEG, hue, hue + MIXER_HUE_MAX_DEG].map(bandColourCss);
  return `linear-gradient(to right, ${stops.join(', ')})`;
}

/**
 * The gain in stops of a colour balance slider's own channel at +-100. It must match `AXIS_STOPS`
 * in `crates/gtp-edit/src/color.rs`; a test compares the two.
 */
export const AXIS_STOPS = 1;

/** Mid grey, as a linear value (the usual 18 percent card): what the colour balance stops tint. */
export const MID_GREY = 0.18;

/** The recipe's colour balance axes. */
export type BalanceAxis = Extract<ColourKey, 'red_cyan' | 'green_magenta' | 'blue_yellow'>;
const BALANCE_AXES: readonly BalanceAxis[] = ['red_cyan', 'green_magenta', 'blue_yellow'];

/** A linear Rec.2020 triple as CSS `rgb()` in sRGB, clipped to the gamut, brightness kept. */
function linearRec2020Css(rgb2020: readonly [number, number, number]): string {
  const [r, g, b] = clippedSrgb(rgb2020).map((c) => Math.round(encode(c) * CHANNEL_MAX));
  return `rgb(${r} ${g} ${b})`;
}

/** Mid grey with the pipeline's gains of a colour balance axis at `a` (-1 to 1) applied. */
function balanceStopCss(axisIndex: number, a: number): string {
  const rgb = [0, 1, 2].map(
    (c) => MID_GREY * 2 ** (c === axisIndex ? a * AXIS_STOPS : (-a * AXIS_STOPS) / 2),
  );
  return linearRec2020Css([rgb[0] ?? 0, rgb[1] ?? 0, rgb[2] ?? 0]);
}

/**
 * A left to right gradient for a colour balance slider: the cast of -100, of 0 (the plain mid grey)
 * and of +100 on a mid grey, by the gains of the pipeline.
 */
export function axisGradientCss(axis: BalanceAxis): string {
  const index = BALANCE_AXES.indexOf(axis);
  const stops = [-1, 0, 1].map((a) => balanceStopCss(index, a));
  return `linear-gradient(to right, ${stops.join(', ')})`;
}
