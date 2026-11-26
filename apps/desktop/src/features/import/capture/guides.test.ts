import { describe, expect, it } from 'vitest';
import type { FilmFormat } from '../../../shared/ipc';
import {
  FIT,
  FORMAT_ASPECT,
  KEY_STEP,
  MIN_SIDE,
  clampRect,
  defaultRect,
  dragCorner,
  gridLines,
  guideKey,
  keyRect,
  moveRect,
  readRect,
  type GuideRect,
} from './guides';

const PICTURE = 640 / 424;

/** The rectangle's shape in pixels: width over height. */
const pixelShape = (r: GuideRect, aspect: number) => (r.w * aspect) / r.h;

describe('default frame lines from the film format', () => {
  const formats: FilmFormat[] = ['35mm', '645', '6x6', '6x7', '6x9'];

  it.each(formats)('%s has the format proportions, centred, in both orientations', (format) => {
    for (const aspect of [PICTURE, 1.5, 4 / 3, 1]) {
      const landscape = defaultRect({
        format,
        frameSize: null,
        orientation: 'landscape',
        pictureAspect: aspect,
      });
      expect(pixelShape(landscape, aspect)).toBeCloseTo(FORMAT_ASPECT[format], 5);
      const portrait = defaultRect({
        format,
        frameSize: null,
        orientation: 'portrait',
        pictureAspect: aspect,
      });
      expect(pixelShape(portrait, aspect)).toBeCloseTo(1 / FORMAT_ASPECT[format], 5);
      for (const r of [landscape, portrait]) {
        expect(r.x + r.w / 2).toBeCloseTo(0.5, 6);
        expect(r.y + r.h / 2).toBeCloseTo(0.5, 6);
        expect(r.w).toBeLessThanOrEqual(FIT + 1e-9);
        expect(r.h).toBeLessThanOrEqual(FIT + 1e-9);
        // The largest that fits: one side reaches 90 %.
        expect(Math.max(r.w, r.h)).toBeCloseTo(FIT, 6);
      }
    }
  });

  it('knows the nominal proportions', () => {
    expect(FORMAT_ASPECT['35mm']).toBeCloseTo(1.5);
    expect(FORMAT_ASPECT['645']).toBeCloseTo(56 / 41.5);
    expect(FORMAT_ASPECT['6x6']).toBe(1);
    expect(FORMAT_ASPECT['6x7']).toBeCloseTo(1.25);
    expect(FORMAT_ASPECT['6x9']).toBeCloseTo(1.5);
  });

  it('a 35 mm frame on a 3:2 picture fills 90 % each way, and a square a smaller part', () => {
    const wide = defaultRect({
      format: '35mm',
      frameSize: null,
      orientation: 'landscape',
      pictureAspect: 1.5,
    });
    expect(wide.w).toBeCloseTo(0.9);
    expect(wide.h).toBeCloseTo(0.9);
    const square = defaultRect({
      format: '6x6',
      frameSize: null,
      orientation: 'landscape',
      pictureAspect: 1.5,
    });
    expect(square.h).toBeCloseTo(0.9);
    expect(square.w).toBeCloseTo(0.6);
  });

  it('falls back to the format when the frame size is missing a side or is not valid', () => {
    const fallback = defaultRect({
      format: '6x7',
      frameSize: null,
      orientation: 'landscape',
      pictureAspect: PICTURE,
    });
    for (const frameSize of [
      { w: 0.5, h: null },
      { w: null, h: 0.5 },
      { w: 0, h: 0.5 },
      { w: Number.NaN, h: 0.5 },
    ]) {
      expect(
        defaultRect({ format: '6x7', frameSize, orientation: 'landscape', pictureAspect: PICTURE }),
      ).toEqual(fallback);
    }
  });
});

describe('default frame lines from a stored frame size', () => {
  const frameSize = { w: 0.6, h: 0.8 };

  it('uses the stored size centred, landscape as it is in pixel terms', () => {
    // 0.6 x 0.8 of a 3:2 picture is 0.9 x 0.8 in units of the picture height: already landscape.
    const r = defaultRect({
      format: '35mm',
      frameSize,
      orientation: 'landscape',
      pictureAspect: 1.5,
    });
    expect(r.w).toBeCloseTo(0.6);
    expect(r.h).toBeCloseTo(0.8);
    expect(r.x).toBeCloseTo(0.2);
    expect(r.y).toBeCloseTo(0.1);
  });

  it('turns to portrait by swapping in pixel terms, not just the fractions', () => {
    const r = defaultRect({
      format: '35mm',
      frameSize,
      orientation: 'portrait',
      pictureAspect: 1.5,
    });
    // Pixel sides 0.9 and 0.8 swap to 0.8 wide and 0.9 tall.
    expect(r.w).toBeCloseTo(0.8 / 1.5);
    expect(r.h).toBeCloseTo(0.9);
    expect(r.x + r.w / 2).toBeCloseTo(0.5);
    expect(r.y + r.h / 2).toBeCloseTo(0.5);
  });

  it('ignores the format when a size is stored and scales a turned frame that would stick out', () => {
    const big = { w: 0.95, h: 0.9 };
    const r = defaultRect({
      format: '6x6',
      frameSize: big,
      orientation: 'portrait',
      pictureAspect: 2,
    });
    expect(r.x).toBeGreaterThanOrEqual(-1e-9);
    expect(r.y).toBeGreaterThanOrEqual(-1e-9);
    expect(r.x + r.w).toBeLessThanOrEqual(1 + 1e-9);
    expect(r.y + r.h).toBeLessThanOrEqual(1 + 1e-9);
    // Still the turned shape: 0.9 wide in pixel terms against 1.9 tall.
    expect(pixelShape(r, 2)).toBeCloseTo(0.9 / 1.9, 5);
  });
});

describe('adjusting', () => {
  const start: GuideRect = { x: 0.2, y: 0.2, w: 0.5, h: 0.4 };

  it('moves and stays inside the picture', () => {
    const moved = moveRect(start, 0.1, -0.1);
    expect(moved.x).toBeCloseTo(0.3);
    expect(moved.y).toBeCloseTo(0.1);
    expect(moved.w).toBe(0.5);
    const far = moveRect(start, 5, 5);
    expect(far.x).toBeCloseTo(0.5);
    expect(far.y).toBeCloseTo(0.6);
    const near = moveRect(start, -5, -5);
    expect(near.x).toBe(0);
    expect(near.y).toBe(0);
  });

  it('moves with the arrow keys by one step', () => {
    expect(keyRect(start, 'ArrowRight', false)?.x).toBeCloseTo(0.2 + KEY_STEP);
    expect(keyRect(start, 'ArrowLeft', false)?.x).toBeCloseTo(0.2 - KEY_STEP);
    expect(keyRect(start, 'ArrowDown', false)?.y).toBeCloseTo(0.2 + KEY_STEP);
    expect(keyRect(start, 'ArrowUp', false)?.y).toBeCloseTo(0.2 - KEY_STEP);
    expect(keyRect(start, 'a', false)).toBeNull();
    expect(keyRect(start, 'Enter', true)).toBeNull();
  });

  it('resizes about the centre with Shift and the arrow keys', () => {
    const wider = keyRect(start, 'ArrowRight', true);
    expect(wider?.w).toBeCloseTo(0.5 + 2 * KEY_STEP);
    expect((wider?.x ?? 0) + (wider?.w ?? 0) / 2).toBeCloseTo(0.45);
    expect(wider?.h).toBeCloseTo(0.4);
    expect(keyRect(start, 'ArrowLeft', true)?.w).toBeCloseTo(0.5 - 2 * KEY_STEP);
    expect(keyRect(start, 'ArrowUp', true)?.h).toBeCloseTo(0.4 + 2 * KEY_STEP);
    expect(keyRect(start, 'ArrowDown', true)?.h).toBeCloseTo(0.4 - 2 * KEY_STEP);
  });

  it('drags a corner while the opposite one stays', () => {
    const se = dragCorner(start, 'se', { x: 0.9, y: 0.8 });
    expect([se.x, se.y, se.w, se.h]).toEqual([0.2, 0.2, expect.closeTo(0.7), expect.closeTo(0.6)]);
    const nw = dragCorner(start, 'nw', { x: 0.1, y: 0.05 });
    expect(nw.x).toBeCloseTo(0.1);
    expect(nw.y).toBeCloseTo(0.05);
    expect(nw.x + nw.w).toBeCloseTo(0.7);
    expect(nw.y + nw.h).toBeCloseTo(0.6);
    const ne = dragCorner(start, 'ne', { x: 0.8, y: 0.1 });
    expect(ne.x).toBeCloseTo(0.2);
    expect(ne.x + ne.w).toBeCloseTo(0.8);
    expect(ne.y).toBeCloseTo(0.1);
    expect(ne.y + ne.h).toBeCloseTo(0.6);
    const sw = dragCorner(start, 'sw', { x: 0.1, y: 0.9 });
    expect(sw.x).toBeCloseTo(0.1);
    expect(sw.x + sw.w).toBeCloseTo(0.7);
    expect(sw.y).toBeCloseTo(0.2);
    expect(sw.y + sw.h).toBeCloseTo(0.9);
  });

  it('clamps: inside the picture, not below the smallest size, not turned inside out', () => {
    const out = dragCorner(start, 'se', { x: 4, y: -3 });
    expect(out.x + out.w).toBeLessThanOrEqual(1);
    expect(out.h).toBeGreaterThanOrEqual(MIN_SIDE);
    const crossed = dragCorner(start, 'se', { x: 0, y: 0 });
    expect(crossed.w).toBeGreaterThanOrEqual(MIN_SIDE);
    expect(crossed.h).toBeGreaterThanOrEqual(MIN_SIDE);
    let tiny = start;
    for (let i = 0; i < 200; i += 1) tiny = keyRect(tiny, 'ArrowLeft', true) ?? tiny;
    expect(tiny.w).toBeCloseTo(MIN_SIDE);
    let huge = start;
    for (let i = 0; i < 400; i += 1) huge = keyRect(huge, 'ArrowRight', true) ?? huge;
    expect(huge.w).toBe(1);
    expect(huge.x).toBe(0);
    expect(clampRect({ x: 0.9, y: 0.9, w: 0.5, h: 2 })).toEqual({ x: 0.5, y: 0, w: 0.5, h: 1 });
    expect(clampRect({ x: Number.NaN, y: 0, w: 0, h: 0.5 }).w).toBe(MIN_SIDE);
  });

  it('keeps the lines apart per format and orientation', () => {
    expect(guideKey('35mm', 'landscape')).toBe('35mm-landscape');
    expect(guideKey('6x6', 'portrait')).toBe('6x6-portrait');
  });

  it('reads a stored rectangle back, or null when it cannot be used', () => {
    expect(readRect({ x: 0.1, y: 0.1, w: 0.5, h: 0.5 })).toEqual({
      x: 0.1,
      y: 0.1,
      w: 0.5,
      h: 0.5,
    });
    expect(readRect({ x: null, y: 0.1, w: 0.5, h: 0.5 })).toBeNull();
    expect(readRect({ x: 0.1, y: 0.1, w: 0, h: 0.5 })).toBeNull();
  });
});

describe('grid lines', () => {
  it('has two inner lines for thirds and nine for the fine grid', () => {
    expect(gridLines('thirds')).toEqual([1 / 3, 2 / 3]);
    expect(gridLines('fine')).toHaveLength(9);
    expect(gridLines('fine')[4]).toBeCloseTo(0.5);
  });
});
