import { describe, expect, it } from 'vitest';
import { makeRecipe } from '../library/fake-ipc';
import {
  SHARPEN_AMOUNT_MAX,
  SHARPEN_DEFAULT,
  SHARPEN_RADIUS_MAX,
  SHARPEN_RADIUS_MIN,
  sharpenOf,
  withSharpen,
} from './recipe-parts';

describe('the sharpening of a recipe', () => {
  it('is off, at a radius of one pixel, when the recipe has none', () => {
    expect(sharpenOf(makeRecipe('x'))).toEqual({ amount: 0, radius_px: 1 });
    expect(sharpenOf({} as never)).toEqual(SHARPEN_DEFAULT);
    expect(SHARPEN_DEFAULT).toEqual({ amount: 0, radius_px: 1 });
  });

  it('reads what is there', () => {
    const recipe = makeRecipe('x', {
      output: { space: 'srgb', sharpen: { amount: 80, radius_px: 1.6 } },
    });
    expect(sharpenOf(recipe)).toEqual({ amount: 80, radius_px: 1.6 });
  });

  it('gives plain numbers inside the ranges, whatever the recipe holds', () => {
    const wild = makeRecipe('x', { output: { sharpen: { amount: 900, radius_px: 0 } } });
    expect(sharpenOf(wild)).toEqual({ amount: SHARPEN_AMOUNT_MAX, radius_px: SHARPEN_RADIUS_MIN });
    const other = makeRecipe('x', { output: { sharpen: { amount: -4, radius_px: 80 } } });
    expect(sharpenOf(other)).toEqual({ amount: 0, radius_px: SHARPEN_RADIUS_MAX });
    // A number that travelled as null counts as missing.
    const nulls = makeRecipe('x', { output: { sharpen: { amount: null, radius_px: null } } });
    expect(sharpenOf(nulls)).toEqual(SHARPEN_DEFAULT);
  });

  it('changes some values and keeps the rest, including the colour space', () => {
    const recipe = makeRecipe('x', {
      output: { space: 'prophoto', sharpen: { amount: 40, radius_px: 2 } },
    });
    const next = withSharpen(recipe, { amount: 90 });
    expect(sharpenOf(next)).toEqual({ amount: 90, radius_px: 2 });
    expect((next.output as { space: string }).space).toBe('prophoto');
    // The recipe it was given is not touched.
    expect(sharpenOf(recipe)).toEqual({ amount: 40, radius_px: 2 });
  });

  it('adds the sharpening to a recipe that has no output block', () => {
    const next = withSharpen({} as never, { radius_px: 2.5 });
    expect(sharpenOf(next)).toEqual({ amount: 0, radius_px: 2.5 });
  });

  it('has the limits of the recipe', () => {
    expect([SHARPEN_AMOUNT_MAX, SHARPEN_RADIUS_MIN, SHARPEN_RADIUS_MAX]).toEqual([150, 0.3, 3]);
  });
});
