import { describe, expect, it } from 'vitest';
import { makeRecipe } from '../library/fake-ipc';
import { NOISE_DEFAULT, NOISE_MAX, noiseOf, withNoise } from './recipe-parts';

describe('the noise reduction of a recipe', () => {
  it('is off when the recipe has none', () => {
    expect(noiseOf(makeRecipe('x'))).toEqual({ luminance: 0, color: 0 });
    expect(noiseOf({} as never)).toEqual(NOISE_DEFAULT);
    expect(NOISE_DEFAULT).toEqual({ luminance: 0, color: 0 });
  });

  it('reads what is there', () => {
    expect(noiseOf(makeRecipe('x', { noise: { luminance: 35, color: 80 } }))).toEqual({
      luminance: 35,
      color: 80,
    });
  });

  it('gives plain numbers inside the range, whatever the recipe holds', () => {
    const wild = makeRecipe('x', { noise: { luminance: 900, color: -4 } });
    expect(noiseOf(wild)).toEqual({ luminance: NOISE_MAX, color: 0 });
    const odd = makeRecipe('x', { noise: { luminance: null, color: Number.NaN } });
    expect(noiseOf(odd)).toEqual({ luminance: 0, color: 0 });
    expect(noiseOf(makeRecipe('x', { noise: { color: 20 } }))).toEqual({
      luminance: 0,
      color: 20,
    });
  });

  it('replaces one amount and keeps the other and the rest of the recipe', () => {
    const recipe = makeRecipe('x', { noise: { luminance: 10, color: 20 } });
    const next = withNoise(recipe, { color: 55 });
    expect(noiseOf(next)).toEqual({ luminance: 10, color: 55 });
    expect(next.geometry).toEqual(recipe.geometry);
    expect(noiseOf(withNoise(makeRecipe('x'), { luminance: 5 }))).toEqual({
      luminance: 5,
      color: 0,
    });
  });
});
