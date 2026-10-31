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
  'exposure_ev' | 'contrast' | 'highlights' | 'shadows' | 'whites' | 'blacks' | 'dehaze';

export type Tone = Record<ToneKey, number>;

/** The tone sliders of a recipe, as plain numbers (0 when missing). */
export function toneOf(recipe: Recipe): Tone {
  const t = recipe.tone;
  return {
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
  return { ...recipe, tone: { ...recipe.tone, ...toneOf(recipe), ...values } } as Recipe;
}

/** The recipe with one tone curve replaced. */
export function withCurve(recipe: Recipe, channel: CurveChannel, points: CurvePoint[]): Recipe {
  const curve = { ...curvesOf(recipe), [channel]: points.map((p) => [...p]) };
  return { ...recipe, tone: { ...recipe.tone, ...toneOf(recipe), curve } } as Recipe;
}

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
