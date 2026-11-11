import { describe, expect, it } from 'vitest';
import { makeRecipe } from '../library/fake-ipc';
import { colourOf, splitToneOf, withColour, withSplitTone } from './recipe-parts';

describe('the split toning part of a recipe', () => {
  it('reads the defaults for a recipe without it, and plain numbers for the rest', () => {
    expect(splitToneOf(makeRecipe('x'))).toEqual({
      highlight_hue: 40,
      highlight_strength: 0,
      shadow_hue: 220,
      shadow_strength: 0,
      balance: 0,
    });
    const odd = makeRecipe('y', { color: { split_tone: { shadow_hue: null, balance: 30 } } });
    expect(splitToneOf(odd)).toMatchObject({ shadow_hue: 220, balance: 30 });
  });

  it('keeps it inside color.split_tone and keeps the rest when one value changes', () => {
    const recipe = makeRecipe('x', {
      color: { temperature: 9, split_tone: { highlight_hue: 10, highlight_strength: 50 } },
    });
    const next = withSplitTone(recipe, { balance: -20 });
    expect(next.color?.split_tone).toEqual({
      highlight_hue: 10,
      highlight_strength: 50,
      shadow_hue: 220,
      shadow_strength: 0,
      balance: -20,
    });
    expect(colourOf(next).temperature).toBe(9);
    // A colour change keeps the split toning.
    expect(withColour(next, { tint: 3 }).color?.split_tone).toEqual(next.color?.split_tone);
    expect(colourOf(next)).not.toHaveProperty('split_tone');
  });
});
