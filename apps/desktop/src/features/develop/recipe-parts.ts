// The parts of a recipe the Base and Invert panel works with, read and changed without touching
// the rest. The generated recipe type allows missing fields and null numbers (a NaN travels as
// null), so reading goes through these and always gives plain numbers.
import type { Recipe } from '../../shared/ipc';

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
