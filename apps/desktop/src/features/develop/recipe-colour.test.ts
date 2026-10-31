import { describe, expect, it } from 'vitest';
import { makeRecipe } from '../library/fake-ipc';
import { LUMA_MIX, colourOf, monoOf, withColour, withMono } from './recipe-parts';

describe('the colour parts of a recipe', () => {
  it('reads plain numbers, with the defaults for what is missing', () => {
    const bare = makeRecipe('x');
    expect(colourOf(bare)).toEqual({
      red_cyan: 0,
      green_magenta: 0,
      blue_yellow: 0,
      temperature: 0,
      tint: 0,
      saturation: 1,
      vibrance: 0,
    });
    expect(monoOf(bare)).toEqual({ enabled: false, mix: LUMA_MIX });
  });

  it('turns a number that travelled as null into the default', () => {
    const recipe = makeRecipe('x', {
      color: { red_cyan: null, tint: 12, saturation: null },
      mono: { enabled: true, mix: [null, 0.5, 0.25] },
    });
    expect(colourOf(recipe)).toMatchObject({ red_cyan: 0, tint: 12, saturation: 1 });
    expect(monoOf(recipe)).toEqual({ enabled: true, mix: [LUMA_MIX[0], 0.5, 0.25] });
  });

  it('changes some sliders and keeps the rest, without touching the recipe it was given', () => {
    const recipe = makeRecipe('x', { color: { red_cyan: 5, temperature: -3 } });
    const next = withColour(recipe, { blue_yellow: 40, red_cyan: 7 });
    expect(colourOf(next)).toMatchObject({
      red_cyan: 7,
      blue_yellow: 40,
      temperature: -3,
      saturation: 1,
    });
    expect(colourOf(recipe).red_cyan).toBe(5);
    expect(colourOf(recipe).blue_yellow).toBe(0);
  });

  it('replaces the black-and-white setting with a copy', () => {
    const recipe = makeRecipe('x');
    const mix: [number, number, number] = [1, 0, 0];
    const next = withMono(recipe, { enabled: true, mix });
    mix[0] = 9;
    expect(monoOf(next)).toEqual({ enabled: true, mix: [1, 0, 0] });
    expect(monoOf(recipe).enabled).toBe(false);
  });
});
