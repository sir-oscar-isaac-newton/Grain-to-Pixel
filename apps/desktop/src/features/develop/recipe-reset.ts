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
  mixerOf,
  monoOf,
  noiseOf,
  sharpenOf,
  splitToneOf,
  toneOf,
  withBase,
  withColour,
  withDust,
  withGeometry,
  withLevels,
  withMixer,
  withMono,
  withNoise,
  withSharpen,
  withSplitTone,
  withTone,
} from './recipe-parts';

/**
 * The Colour panel: the colour sliders, the black-and-white setting, the colour mixer and the
 * split toning.
 */
export function resetColour(recipe: Recipe, auto: Recipe): Recipe {
  return withSplitTone(
    withMixer(withMono(withColour(recipe, colourOf(auto)), monoOf(auto)), mixerOf(auto)),
    splitToneOf(auto),
  );
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

/** The Detail panel: the noise reduction and the output sharpening (amount and radius). */
export function resetDetail(recipe: Recipe, auto: Recipe): Recipe {
  return withSharpen(withNoise(recipe, noiseOf(auto)), sharpenOf(auto));
}
