// What each panel's "Reset" puts back: the groups of the panel, taken from the automatic recipe.
// The generated recipe type allows missing fields, so everything goes through the helpers of
// recipe-parts and gives plain values.
import type { Recipe } from '../../shared/ipc';
import {
  baseOf,
  colourOf,
  curvesOf,
  dustOf,
  geometryOf,
  levelsOf,
  monoOf,
  sharpenOf,
  toneOf,
  withBase,
  withColour,
  withDust,
  withGeometry,
  withLevels,
  withMono,
  withSharpen,
  withTone,
} from './recipe-parts';

/** The Colour panel: the colour sliders and the black-and-white setting. */
export function resetColour(recipe: Recipe, auto: Recipe): Recipe {
  return withMono(withColour(recipe, colourOf(auto)), monoOf(auto));
}

/** The Tone panel: the tone sliders and all four curves. */
export function resetTone(recipe: Recipe, auto: Recipe): Recipe {
  const curve = curvesOf(auto);
  const next = withTone(recipe, toneOf(auto));
  return { ...next, tone: { ...next.tone, curve } } as Recipe;
}

/**
 * The Geometry panel: the whole geometry (crop, straighten angle, quarter turns and flips), so the
 * frame is as the automatic conversion found it. The aspect-ratio lock is not part of the recipe
 * and stays.
 */
export function resetGeometry(recipe: Recipe, auto: Recipe): Recipe {
  return withGeometry(recipe, geometryOf(auto));
}

/** The Base and Invert panel: the film base (source and values) and the black and white points. */
export function resetBaseInvert(recipe: Recipe, auto: Recipe): Recipe {
  return withLevels(withBase(recipe, baseOf(auto)), levelsOf(auto));
}

/** The Dust panel: on or off, the threshold and the growth. */
export function resetDust(recipe: Recipe, auto: Recipe): Recipe {
  return withDust(recipe, dustOf(auto));
}

/** The Sharpen panel: amount and radius. */
export function resetSharpen(recipe: Recipe, auto: Recipe): Recipe {
  return withSharpen(recipe, sharpenOf(auto));
}
