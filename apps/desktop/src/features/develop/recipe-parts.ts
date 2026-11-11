// The parts of a recipe the Base and Invert panel works with, read and changed without touching
// the rest. The generated recipe type allows missing fields and null numbers (a NaN travels as
// null), so reading goes through these and always gives plain numbers.
import { identityCurve, type CurveChannel, type CurvePoint, type CurveSet } from '@gtp/ui';
import type { Recipe } from '../../shared/ipc';
import { clampCrop, type CropRect } from './crop';

export type Triple = [number, number, number];
export type BaseSource = 'roll' | 'manual';
export type LevelKind = 'black' | 'white';
export type Channel = 0 | 1 | 2;

export interface BaseSetting {
  source: BaseSource;
  rgb: Triple;
}

export interface Levels {
  black: Triple;
  white: Triple;
}

const clampTo = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));

const plain = (value: number | null | undefined, fallback: number): number =>
  typeof value === 'number' && Number.isFinite(value) ? value : fallback;

/** Three numbers from a possibly incomplete list, with `fallback` for any that are missing. */
export function triple(
  values: ReadonlyArray<number | null> | null | undefined,
  fallback: number,
): Triple {
  return [plain(values?.[0], fallback), plain(values?.[1], fallback), plain(values?.[2], fallback)];
}

/** Where the film base comes from, and its value. */
export function baseOf(recipe: Recipe): BaseSetting {
  return {
    source: recipe.base?.source === 'manual' ? 'manual' : 'roll',
    rgb: triple(recipe.base?.rgb, 1),
  };
}

/** The black and white points of an `invert` block (the recipe's, or one the backend returned). */
export function levelsIn(invert: Partial<Recipe['invert']> | null | undefined): Levels {
  return { black: triple(invert?.black, 0), white: triple(invert?.white, 1) };
}

/** The black and white points of the inversion, per channel. */
export function levelsOf(recipe: Recipe): Levels {
  return levelsIn(recipe.invert);
}

export type ColourKey =
  'red_cyan' | 'green_magenta' | 'blue_yellow' | 'temperature' | 'tint' | 'saturation' | 'vibrance';

export type Colour = Record<ColourKey, number>;

export interface MonoSetting {
  enabled: boolean;
  mix: Triple;
}

/** The default black-and-white mix: the Rec.2020 luma weights. */
export const LUMA_MIX: Triple = [0.2627, 0.678, 0.0593];

/** The colour sliders of a recipe, as plain numbers (saturation 1 and the rest 0 when missing). */
export function colourOf(recipe: Recipe): Colour {
  const c = recipe.color;
  return {
    red_cyan: plain(c?.red_cyan, 0),
    green_magenta: plain(c?.green_magenta, 0),
    blue_yellow: plain(c?.blue_yellow, 0),
    temperature: plain(c?.temperature, 0),
    tint: plain(c?.tint, 0),
    saturation: plain(c?.saturation, 1),
    vibrance: plain(c?.vibrance, 0),
  };
}

export type SplitToneKey =
  'highlight_hue' | 'highlight_strength' | 'shadow_hue' | 'shadow_strength' | 'balance';

export type SplitToneSetting = Record<SplitToneKey, number>;

/** The split toning values when a recipe has none: both strengths 0 (off), the usual hues. */
export const SPLIT_TONE_DEFAULT: SplitToneSetting = {
  highlight_hue: 40,
  highlight_strength: 0,
  shadow_hue: 220,
  shadow_strength: 0,
  balance: 0,
};

/** The split toning of a recipe (`color.split_tone`), as plain numbers (the defaults when missing). */
export function splitToneOf(recipe: Recipe): SplitToneSetting {
  const s = recipe.color?.split_tone;
  return {
    highlight_hue: plain(s?.highlight_hue, SPLIT_TONE_DEFAULT.highlight_hue),
    highlight_strength: plain(s?.highlight_strength, SPLIT_TONE_DEFAULT.highlight_strength),
    shadow_hue: plain(s?.shadow_hue, SPLIT_TONE_DEFAULT.shadow_hue),
    shadow_strength: plain(s?.shadow_strength, SPLIT_TONE_DEFAULT.shadow_strength),
    balance: plain(s?.balance, SPLIT_TONE_DEFAULT.balance),
  };
}

/** The recipe with some of its split toning replaced, the rest of the colour block kept. */
export function withSplitTone(recipe: Recipe, values: Partial<SplitToneSetting>): Recipe {
  return {
    ...recipe,
    color: { ...recipe.color, split_tone: { ...splitToneOf(recipe), ...values } },
  } as Recipe;
}

export type MixerMode = 'hue' | 'saturation' | 'lightness';

/** The three lists of the colour mixer, eight values each, in the order of {@link MIXER_BANDS}. */
export type MixerSetting = Record<MixerMode, number[]>;

/** The modes of the colour mixer, in the order they are offered. */
export const MIXER_MODES: readonly MixerMode[] = ['hue', 'saturation', 'lightness'];

/** The hue bands of the colour mixer, in the order of the recipe's lists (index 0 to 7). */
export const MIXER_BANDS = [
  'red',
  'orange',
  'yellow',
  'green',
  'aqua',
  'blue',
  'purple',
  'magenta',
] as const;

const mixerList = (values: ReadonlyArray<number | null> | null | undefined): number[] =>
  MIXER_BANDS.map((_, index) => plain(values?.[index], 0));

/** The colour mixer of a recipe (`color.mixer`), as plain numbers (zeros when missing). */
export function mixerOf(recipe: Recipe): MixerSetting {
  const m = recipe.color?.mixer;
  return {
    hue: mixerList(m?.hue),
    saturation: mixerList(m?.saturation),
    lightness: mixerList(m?.lightness),
  };
}

/** The recipe with some of its mixer lists replaced, the rest of the colour block kept. */
export function withMixer(recipe: Recipe, values: Partial<MixerSetting>): Recipe {
  return {
    ...recipe,
    color: { ...recipe.color, mixer: { ...mixerOf(recipe), ...values } },
  } as Recipe;
}

/** The recipe with one value of the colour mixer set: the band at `index` of the list `mode`. */
export function withMixerValue(
  recipe: Recipe,
  mode: MixerMode,
  index: number,
  value: number,
): Recipe {
  const list = [...mixerOf(recipe)[mode]];
  list[index] = value;
  return withMixer(recipe, { [mode]: list });
}

/** The black-and-white setting of a recipe. */
export function monoOf(recipe: Recipe): MonoSetting {
  const mono = recipe.mono;
  const mix = mono?.mix;
  return {
    enabled: mono?.enabled === true,
    mix: [plain(mix?.[0], LUMA_MIX[0]), plain(mix?.[1], LUMA_MIX[1]), plain(mix?.[2], LUMA_MIX[2])],
  };
}

/** The recipe with some of its colour sliders replaced, the rest of the colour block kept. */
export function withColour(recipe: Recipe, values: Partial<Colour>): Recipe {
  return { ...recipe, color: { ...recipe.color, ...colourOf(recipe), ...values } } as Recipe;
}

/** The recipe with its black-and-white setting replaced. */
export function withMono(recipe: Recipe, mono: MonoSetting): Recipe {
  return { ...recipe, mono: { enabled: mono.enabled, mix: [...mono.mix] } } as Recipe;
}

export type ToneKey =
  | 'exposure_ev'
  | 'contrast'
  | 'highlights'
  | 'shadows'
  | 'whites'
  | 'blacks'
  | 'dehaze'
  | 'vignette'
  | 'vignette_midpoint';

export type Tone = Record<ToneKey, number>;

/** The vignette's own values when a recipe has none: no effect, midpoint in the middle. */
export const VIGNETTE_DEFAULT = { amount: 0, midpoint: 50 };

/**
 * The tone sliders of a recipe, as plain numbers (0 when missing, the vignette midpoint 50). The
 * vignette sliders are the recipe's `tone.vignette.amount` and `tone.vignette.midpoint`.
 */
export function toneOf(recipe: Recipe): Tone {
  const t = recipe.tone;
  return {
    vignette: plain(t?.vignette?.amount, VIGNETTE_DEFAULT.amount),
    vignette_midpoint: plain(t?.vignette?.midpoint, VIGNETTE_DEFAULT.midpoint),
    exposure_ev: plain(t?.exposure_ev, 0),
    contrast: plain(t?.contrast, 0),
    highlights: plain(t?.highlights, 0),
    shadows: plain(t?.shadows, 0),
    whites: plain(t?.whites, 0),
    blacks: plain(t?.blacks, 0),
    dehaze: plain(t?.dehaze, 0),
  };
}

const plainPoints = (
  points: ReadonlyArray<ReadonlyArray<number | null>> | null | undefined,
): CurvePoint[] =>
  (points ?? []).flatMap((p) => {
    const [x, y] = [p[0], p[1]];
    return typeof x === 'number' && typeof y === 'number' && Number.isFinite(x + y)
      ? [[x, y] as CurvePoint]
      : [];
  });

/** The four tone curves of a recipe; an empty list is a curve that changes nothing. */
export function curvesOf(recipe: Recipe): CurveSet {
  const c = recipe.tone?.curve;
  const master = plainPoints(c?.master);
  return {
    // A missing master is the identity line, as the recipe's default is.
    master: c?.master ? master : identityCurve(),
    r: plainPoints(c?.r),
    g: plainPoints(c?.g),
    b: plainPoints(c?.b),
  };
}

/** The recipe with some of its tone sliders replaced, the rest of the tone block kept. */
export function withTone(recipe: Recipe, values: Partial<Tone>): Recipe {
  const { vignette, vignette_midpoint: midpoint, ...sliders } = { ...toneOf(recipe), ...values };
  return {
    ...recipe,
    tone: { ...recipe.tone, ...sliders, vignette: { amount: vignette, midpoint } },
  } as Recipe;
}

/** The recipe with one tone curve replaced. */
export function withCurve(recipe: Recipe, channel: CurveChannel, points: CurvePoint[]): Recipe {
  const curve = { ...curvesOf(recipe), [channel]: points.map((p) => [...p]) };
  const base = withTone(recipe, {});
  return { ...base, tone: { ...base.tone, curve } } as Recipe;
}

/** The output sharpening of a recipe: how much, in percent, and the radius in exported pixels. */
export interface SharpenSetting {
  amount: number;
  radius_px: number;
}

/** The range of each value, and the radius used when none is chosen (the recipe's own limits). */
export const SHARPEN_AMOUNT_MAX = 150;
export const SHARPEN_RADIUS_MIN = 0.3;
export const SHARPEN_RADIUS_MAX = 3;
export const SHARPEN_DEFAULT: SharpenSetting = { amount: 0, radius_px: 1 };

/** The sharpening of a recipe as plain numbers, inside their ranges (off when missing). */
export function sharpenOf(recipe: Recipe): SharpenSetting {
  const s = recipe.output?.sharpen;
  return {
    amount: clampTo(plain(s?.amount, SHARPEN_DEFAULT.amount), 0, SHARPEN_AMOUNT_MAX),
    radius_px: clampTo(
      plain(s?.radius_px, SHARPEN_DEFAULT.radius_px),
      SHARPEN_RADIUS_MIN,
      SHARPEN_RADIUS_MAX,
    ),
  };
}

/** The recipe with some of its sharpening replaced; the colour space of the output is kept. */
export function withSharpen(recipe: Recipe, values: Partial<SharpenSetting>): Recipe {
  return {
    ...recipe,
    output: { ...recipe.output, sharpen: { ...sharpenOf(recipe), ...values } },
  } as Recipe;
}

/** The dust removal of a recipe: on or off, the threshold (0 to 1) and the growth in pixels. */
export interface DustSetting {
  enabled: boolean;
  threshold: number;
  grow_px: number;
}

export const DUST_GROW_MAX = 10;
export const DUST_DEFAULT: DustSetting = { enabled: true, threshold: 0.5, grow_px: 2 };

/** The dust removal of a recipe as plain numbers, inside their ranges (the defaults when missing). */
export function dustOf(recipe: Recipe): DustSetting {
  const d = recipe.dust;
  return {
    enabled: d?.enabled === undefined || d.enabled === null ? DUST_DEFAULT.enabled : d.enabled,
    threshold: clampTo(plain(d?.threshold, DUST_DEFAULT.threshold), 0, 1),
    grow_px: Math.round(clampTo(plain(d?.grow_px, DUST_DEFAULT.grow_px), 0, DUST_GROW_MAX)),
  };
}

/** The recipe with some of its dust removal replaced. */
export function withDust(recipe: Recipe, values: Partial<DustSetting>): Recipe {
  return { ...recipe, dust: { ...dustOf(recipe), ...values } } as Recipe;
}

/** The slider's sensitivity in percent for a threshold: a lower threshold finds more dust. */
export const sensitivityOf = (threshold: number): number => Math.round((1 - threshold) * 100);

/** The threshold for a sensitivity in percent. */
export const thresholdOf = (sensitivity: number): number => Math.round(100 - sensitivity) / 100;
export interface GeometrySetting {
  crop: CropRect;
  angle_deg: number;
  /** Quarter turns clockwise, 0 to 3. */
  rot90: number;
  flip_h: boolean;
  flip_v: boolean;
}

/** The crop, straighten angle, turns and flips of a recipe, as plain values. */
export function geometryOf(recipe: Recipe): GeometrySetting {
  const g = recipe.geometry;
  return {
    crop: clampCrop({
      x: plain(g?.crop?.x, 0),
      y: plain(g?.crop?.y, 0),
      w: plain(g?.crop?.w, 1),
      h: plain(g?.crop?.h, 1),
    }),
    angle_deg: plain(g?.angle_deg, 0),
    rot90: ((Math.round(plain(g?.rot90, 0)) % 4) + 4) % 4,
    flip_h: g?.flip_h === true,
    flip_v: g?.flip_v === true,
  };
}

/** The recipe with some of its geometry replaced, the rest kept. */
export function withGeometry(recipe: Recipe, values: Partial<GeometrySetting>): Recipe {
  return {
    ...recipe,
    geometry: { ...recipe.geometry, ...geometryOf(recipe), ...values },
  } as Recipe;
}

/** The recipe with its film base replaced. */
export function withBase(recipe: Recipe, base: BaseSetting): Recipe {
  return { ...recipe, base: { source: base.source, rgb: [...base.rgb] } } as Recipe;
}

/** The recipe with both sets of levels replaced. */
export function withLevels(recipe: Recipe, levels: Levels): Recipe {
  return { ...recipe, invert: { black: [...levels.black], white: [...levels.white] } } as Recipe;
}

/** The recipe with one channel of the black or white point set. */
export function withLevel(
  recipe: Recipe,
  kind: LevelKind,
  channel: Channel,
  value: number,
): Recipe {
  const levels = levelsOf(recipe);
  const row: Triple = [...levels[kind]];
  row[channel] = value;
  return withLevels(recipe, { ...levels, [kind]: row });
}
