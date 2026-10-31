import { describe, expect, it } from 'vitest';
import type { Recipe } from '../../shared/ipc';
import { makeRecipe } from '../library/fake-ipc';
import { baseOf, levelsOf, triple, withBase, withLevel, withLevels } from './recipe-parts';

const recipe = (parts: Record<string, unknown>) => parts as unknown as Recipe;

describe('recipe parts', () => {
  it('reads plain numbers from lists that are missing, short or hold null', () => {
    expect(triple([1, 2, 3], 0)).toEqual([1, 2, 3]);
    expect(triple(undefined, 1)).toEqual([1, 1, 1]);
    expect(triple(null, 0.5)).toEqual([0.5, 0.5, 0.5]);
    expect(triple([null, 2, Number.NaN], 9)).toEqual([9, 2, 9]);
    expect(triple([1], 0)).toEqual([1, 0, 0]);
  });

  it('reads the base: where it comes from and its value, with defaults for a bare recipe', () => {
    expect(baseOf(makeRecipe('x'))).toEqual({ source: 'roll', rgb: [1, 1, 1] });
    expect(baseOf(makeRecipe('x', { base: { source: 'manual', rgb: [0.8, 0.5, 0.3] } }))).toEqual({
      source: 'manual',
      rgb: [0.8, 0.5, 0.3],
    });
    expect(baseOf(recipe({}))).toEqual({ source: 'roll', rgb: [1, 1, 1] });
    expect(baseOf(recipe({ base: { source: 'nonsense', rgb: [null, 2, 3] } }))).toEqual({
      source: 'roll',
      rgb: [1, 2, 3],
    });
  });

  it('reads the levels, with the defaults of a bare recipe', () => {
    expect(levelsOf(makeRecipe('x'))).toEqual({ black: [0.1, 0.2, 0.3], white: [0.9, 0.8, 0.7] });
    expect(levelsOf(recipe({}))).toEqual({ black: [0, 0, 0], white: [1, 1, 1] });
  });

  it('replaces the base without touching anything else, or the recipe it came from', () => {
    const before = makeRecipe('x');
    const after = withBase(before, { source: 'manual', rgb: [0.7, 0.6, 0.5] });
    expect(baseOf(after)).toEqual({ source: 'manual', rgb: [0.7, 0.6, 0.5] });
    expect(after).toMatchObject({
      tag: 'x',
      geometry: (before as unknown as { geometry: unknown }).geometry,
    });
    expect(levelsOf(after)).toEqual(levelsOf(before));
    expect(baseOf(before).source).toBe('roll');
  });

  it('replaces the levels, or one channel of one of them', () => {
    const before = makeRecipe('x');
    const all = withLevels(before, { black: [0.01, 0.02, 0.03], white: [1.1, 1.2, 1.3] });
    expect(levelsOf(all)).toEqual({ black: [0.01, 0.02, 0.03], white: [1.1, 1.2, 1.3] });
    expect(baseOf(all)).toEqual(baseOf(before));

    const one = withLevel(before, 'white', 1, 0.55);
    expect(levelsOf(one)).toEqual({ black: [0.1, 0.2, 0.3], white: [0.9, 0.55, 0.7] });
    const other = withLevel(one, 'black', 2, 0);
    expect(levelsOf(other)).toEqual({ black: [0.1, 0.2, 0], white: [0.9, 0.55, 0.7] });
    expect(levelsOf(before).white[1]).toBe(0.8);
  });
});
