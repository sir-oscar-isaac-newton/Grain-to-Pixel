import { describe, expect, it } from 'vitest';
import { makeRecipe } from '../library/fake-ipc';
import {
  colourOf,
  MIXER_BANDS,
  MIXER_MODES,
  mixerOf,
  splitToneOf,
  withColour,
  withMixer,
  withMixerValue,
} from './recipe-parts';

const ZEROS = [0, 0, 0, 0, 0, 0, 0, 0];

describe('the colour mixer part of a recipe', () => {
  it('names the eight bands in the order of the recipe and the three modes', () => {
    expect(MIXER_BANDS).toEqual([
      'red',
      'orange',
      'yellow',
      'green',
      'aqua',
      'blue',
      'purple',
      'magenta',
    ]);
    expect(MIXER_MODES).toEqual(['hue', 'saturation', 'lightness']);
  });

  it('reads zeros for a recipe without it, and plain numbers for the rest', () => {
    expect(mixerOf(makeRecipe('x'))).toEqual({ hue: ZEROS, saturation: ZEROS, lightness: ZEROS });
    const odd = makeRecipe('y', {
      color: { mixer: { hue: [5, null, 7], saturation: [0, 0, 0, 0, 0, 0, 0, 80] } },
    });
    expect(mixerOf(odd)).toEqual({
      hue: [5, 0, 7, 0, 0, 0, 0, 0],
      saturation: [0, 0, 0, 0, 0, 0, 0, 80],
      lightness: ZEROS,
    });
  });

  it('keeps it inside color.mixer and keeps the rest when one value changes', () => {
    const recipe = makeRecipe('x', {
      color: { temperature: 9, split_tone: { highlight_hue: 10 }, mixer: { hue: [10] } },
    });
    const next = withMixerValue(recipe, 'lightness', 6, -40);
    expect(next.color?.mixer).toEqual({
      hue: [10, 0, 0, 0, 0, 0, 0, 0],
      saturation: ZEROS,
      lightness: [0, 0, 0, 0, 0, 0, -40, 0],
    });
    expect(colourOf(next).temperature).toBe(9);
    expect(splitToneOf(next).highlight_hue).toBe(10);
    // A colour change keeps the mixer.
    expect(withColour(next, { tint: 3 }).color?.mixer).toEqual(next.color?.mixer);
    expect(colourOf(next)).not.toHaveProperty('mixer');
    // Replacing a whole list leaves the others.
    const swapped = withMixer(next, { saturation: [1, 2, 3, 4, 5, 6, 7, 8] });
    expect(mixerOf(swapped).saturation).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
    expect(mixerOf(swapped).lightness).toEqual(mixerOf(next).lightness);
  });
});
