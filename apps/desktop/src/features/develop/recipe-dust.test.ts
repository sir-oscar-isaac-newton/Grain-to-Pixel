import { describe, expect, it } from 'vitest';
import { makeRecipe } from '../library/fake-ipc';
import {
  DUST_DEFAULT,
  DUST_GROW_MAX,
  dustOf,
  sensitivityOf,
  thresholdOf,
  withDust,
} from './recipe-parts';

describe('the dust removal of a recipe', () => {
  it('is on, at a threshold of a half and a growth of two pixels, when the recipe has none', () => {
    expect(DUST_DEFAULT).toEqual({ enabled: true, threshold: 0.5, grow_px: 2 });
    expect(dustOf({} as never)).toEqual(DUST_DEFAULT);
    expect(dustOf({ dust: { enabled: null, threshold: null, grow_px: null } } as never)).toEqual(
      DUST_DEFAULT,
    );
  });

  it('reads what is there and keeps it inside the ranges the backend clamps to', () => {
    expect(
      dustOf(makeRecipe('x', { dust: { enabled: false, threshold: 0.2, grow_px: 6 } })),
    ).toEqual({ enabled: false, threshold: 0.2, grow_px: 6 });
    expect(dustOf(makeRecipe('x', { dust: { enabled: true, threshold: 7, grow_px: 99 } }))).toEqual(
      {
        enabled: true,
        threshold: 1,
        grow_px: DUST_GROW_MAX,
      },
    );
    expect(
      dustOf(makeRecipe('x', { dust: { enabled: true, threshold: -1, grow_px: -3 } })),
    ).toEqual({ enabled: true, threshold: 0, grow_px: 0 });
  });

  it('replaces only what it is given, and keeps the rest of the recipe', () => {
    const recipe = makeRecipe('x', { dust: { enabled: true, threshold: 0.3, grow_px: 4 } });
    const next = withDust(recipe, { grow_px: 5 });
    expect(dustOf(next)).toEqual({ enabled: true, threshold: 0.3, grow_px: 5 });
    expect({ ...next, dust: undefined }).toEqual({ ...recipe, dust: undefined });
  });

  it('turns the threshold into a sensitivity and back without noise', () => {
    expect(sensitivityOf(0.5)).toBe(50);
    expect(sensitivityOf(0)).toBe(100);
    expect(sensitivityOf(1)).toBe(0);
    for (let percent = 0; percent <= 100; percent += 1) {
      expect(sensitivityOf(thresholdOf(percent))).toBe(percent);
    }
    expect(thresholdOf(71)).toBe(0.29);
  });
});
