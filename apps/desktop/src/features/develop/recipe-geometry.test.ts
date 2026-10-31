import { describe, expect, it } from 'vitest';
import { makeRecipe } from '../library/fake-ipc';
import { geometryOf, withGeometry } from './recipe-parts';

describe('the geometry parts of a recipe', () => {
  it('reads plain values', () => {
    expect(geometryOf(makeRecipe('x'))).toEqual({
      crop: { x: 0.1, y: 0.1, w: 0.8, h: 0.8 },
      angle_deg: 1.5,
      rot90: 1,
      flip_h: true,
      flip_v: false,
    });
  });

  it('gives the whole scan, no turn and no flip for a recipe with none', () => {
    expect(geometryOf({} as never)).toEqual({
      crop: { x: 0, y: 0, w: 1, h: 1 },
      angle_deg: 0,
      rot90: 0,
      flip_h: false,
      flip_v: false,
    });
  });

  it('turns numbers that travelled as null into the defaults and keeps the turn in 0 to 3', () => {
    const recipe = makeRecipe('x', {
      geometry: {
        crop: { x: null, y: 0.2, w: 0.5, h: 0.5 },
        angle_deg: null,
        rot90: -1,
      },
    });
    expect(geometryOf(recipe)).toMatchObject({
      crop: { x: 0, y: 0.2, w: 0.5, h: 0.5 },
      angle_deg: 0,
      rot90: 3,
    });
    // A missing height is the whole height, which pulls the crop back to the top.
    expect(
      geometryOf(makeRecipe('x', { geometry: { crop: { y: 0.2, h: null } } })).crop,
    ).toMatchObject({
      y: 0,
      h: 1,
    });
    expect(geometryOf(makeRecipe('x', { geometry: { rot90: 6 } })).rot90).toBe(2);
  });

  it('changes some values and keeps the rest, without touching the recipe it was given', () => {
    const recipe = makeRecipe('x');
    const next = withGeometry(recipe, { rot90: 2, crop: { x: 0, y: 0, w: 1, h: 1 } });
    expect(geometryOf(next)).toMatchObject({
      rot90: 2,
      angle_deg: 1.5,
      flip_h: true,
      crop: { x: 0, y: 0, w: 1, h: 1 },
    });
    expect(geometryOf(recipe).rot90).toBe(1);
    expect(geometryOf(recipe).crop.w).toBe(0.8);
  });
});
