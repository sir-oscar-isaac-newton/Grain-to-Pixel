import { describe, expect, it } from 'vitest';
import { makeRecipe } from '../library/fake-ipc';
import { curvesOf, toneOf, withCurve, withTone } from './recipe-parts';

describe('the tone parts of a recipe', () => {
  it('reads plain numbers, 0 for what is missing, and the straight line for a missing master', () => {
    const bare = makeRecipe('x');
    expect(toneOf(bare)).toEqual({
      exposure_ev: 0,
      contrast: 0,
      highlights: 0,
      shadows: 0,
      whites: 0,
      blacks: 0,
      dehaze: 0,
      vignette: 0,
      vignette_midpoint: 50,
    });
    expect(curvesOf(bare)).toEqual({
      master: [
        [0, 0],
        [1, 1],
      ],
      r: [],
      g: [],
      b: [],
    });
  });

  it('turns a number that travelled as null into 0 and drops points that are not numbers', () => {
    const recipe = makeRecipe('x', {
      tone: {
        exposure_ev: null,
        contrast: 12,
        curve: {
          master: [
            [0, 0],
            [null, 0.5],
            [1, 1],
          ],
          r: [[0.2, null]],
        },
      },
    });
    expect(toneOf(recipe)).toMatchObject({ exposure_ev: 0, contrast: 12 });
    expect(curvesOf(recipe).master).toEqual([
      [0, 0],
      [1, 1],
    ]);
    expect(curvesOf(recipe).r).toEqual([]);
  });

  it('changes some sliders and keeps the rest, including the curves', () => {
    const recipe = makeRecipe('x', {
      tone: {
        contrast: 5,
        curve: {
          master: [
            [0, 0.1],
            [1, 1],
          ],
          r: [],
          g: [],
          b: [],
        },
      },
    });
    const next = withTone(recipe, { exposure_ev: 0.5, dehaze: -20 });
    expect(toneOf(next)).toMatchObject({ exposure_ev: 0.5, dehaze: -20, contrast: 5 });
    expect(curvesOf(next).master[0]).toEqual([0, 0.1]);
    expect(toneOf(recipe).exposure_ev).toBe(0);
  });

  it('keeps the vignette inside tone.vignette, and keeps the other half when one changes', () => {
    const recipe = makeRecipe('x', { tone: { vignette: { amount: -30, midpoint: 70 } } });
    expect(toneOf(recipe)).toMatchObject({ vignette: -30, vignette_midpoint: 70 });
    const next = withTone(recipe, { vignette: 20 });
    expect(next.tone?.vignette).toEqual({ amount: 20, midpoint: 70 });
    expect(next.tone).not.toHaveProperty('vignette_midpoint');
    const bare = withTone(makeRecipe('y'), { vignette_midpoint: 10 });
    expect(bare.tone?.vignette).toEqual({ amount: 0, midpoint: 10 });
    // A curve change keeps it too.
    const curved = withCurve(recipe, 'g', [
      [0, 0],
      [1, 1],
    ]);
    expect(curved.tone?.vignette).toEqual({ amount: -30, midpoint: 70 });
  });

  it('replaces one curve with a copy and keeps the others', () => {
    const recipe = makeRecipe('x', { tone: { contrast: 5 } });
    const points: Array<[number, number]> = [
      [0, 0],
      [0.5, 0.7],
      [1, 1],
    ];
    const next = withCurve(recipe, 'g', points);
    points[1] = [0.5, 0.1];
    expect(curvesOf(next).g[1]).toEqual([0.5, 0.7]);
    expect(curvesOf(next).master).toEqual(curvesOf(recipe).master);
    expect(toneOf(next).contrast).toBe(5);
    expect(curvesOf(recipe).g).toEqual([]);
  });
});
