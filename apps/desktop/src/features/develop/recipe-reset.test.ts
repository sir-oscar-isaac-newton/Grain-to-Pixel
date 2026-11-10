import { describe, expect, it } from 'vitest';
import { makeRecipe } from '../library/fake-ipc';
import {
  resetBaseInvert,
  resetColour,
  resetDust,
  resetGeometry,
  resetSharpen,
  resetTone,
} from './recipe-reset';

const auto = makeRecipe('auto', {
  geometry: {
    crop: { x: 0, y: 0, w: 0.5, h: 0.5 },
    angle_deg: 0,
    rot90: 0,
    flip_h: false,
    flip_v: false,
  },
  base: { source: 'roll', rgb: [0.9, 0.8, 0.7] },
  invert: { black: [0.01, 0.02, 0.03], white: [0.5, 0.6, 0.7] },
  color: { saturation: 1.1 },
  tone: {
    curve: {
      master: [
        [0, 0.1],
        [1, 1],
      ],
    },
  },
  output: { srgb: true, sharpen: { amount: 20, radius_px: 0.5 } },
  dust: { enabled: false, threshold: 0.3, grow_px: 1 },
});
const edited = makeRecipe('saved', {
  color: { red_cyan: 40, saturation: 1.5 },
  mono: { enabled: true, mix: [1, 0, 0] },
  tone: {
    contrast: 30,
    exposure_ev: 1,
    curve: {
      r: [
        [0, 0.2],
        [1, 1],
      ],
    },
  },
  output: { space: 'adobe', sharpen: { amount: 100, radius_px: 2 } },
});

describe('what a panel reset puts back', () => {
  it('Colour: the sliders and the black-and-white setting, nothing else', () => {
    const next = resetColour(edited, auto) as never as Record<string, Record<string, unknown>>;
    expect(next.color).toMatchObject({ red_cyan: 0, saturation: 1.1 });
    expect(next.mono).toMatchObject({ enabled: false });
    expect(next.tone).toEqual(edited.tone);
    expect(next.geometry).toEqual(edited.geometry);
  });

  it('Tone: the sliders and all the curves, nothing else', () => {
    const next = resetTone(edited, auto) as never as Record<string, Record<string, unknown>>;
    expect(next.tone).toMatchObject({ contrast: 0, exposure_ev: 0 });
    expect((next.tone?.curve as Record<string, unknown>).master).toEqual([
      [0, 0.1],
      [1, 1],
    ]);
    expect((next.tone?.curve as Record<string, unknown>).r).toEqual([]);
    expect(next.color).toEqual(edited.color);
  });

  it('Geometry: crop, angle, turns and flips', () => {
    const next = resetGeometry(edited, auto) as never as Record<string, unknown>;
    expect(next.geometry).toEqual({
      crop: { x: 0, y: 0, w: 0.5, h: 0.5 },
      angle_deg: 0,
      rot90: 0,
      flip_h: false,
      flip_v: false,
    });
  });

  it('Film base: the base and the levels', () => {
    const next = resetBaseInvert(edited, auto) as never as Record<string, unknown>;
    expect(next.base).toEqual({ source: 'roll', rgb: [0.9, 0.8, 0.7] });
    expect(next.invert).toEqual({ black: [0.01, 0.02, 0.03], white: [0.5, 0.6, 0.7] });
  });

  it('Dust: on or off, the threshold and the growth', () => {
    const next = resetDust(edited, auto) as never as Record<string, unknown>;
    expect(next.dust).toEqual({ enabled: false, threshold: 0.3, grow_px: 1 });
  });

  it('Sharpen: the amount and radius, and the output space stays', () => {
    const next = resetSharpen(edited, auto) as never as Record<string, Record<string, unknown>>;
    expect(next.output).toMatchObject({ space: 'adobe', sharpen: { amount: 20, radius_px: 0.5 } });
  });
});
