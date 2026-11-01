import { describe, expect, it } from 'vitest';
import { FULL_CROP, MIN_CROP, type CropRect, type Handle } from './crop';
import {
  FREE_RATIO,
  MAX_RATIO,
  customRatio,
  finishedRatio,
  fitInside,
  fitLargest,
  fractionRatio,
  moveEdgeLocked,
  ratioInScan,
  ratioOfCrop,
  resizeLocked,
  type RatioState,
  type ScanSize,
} from './crop-ratio';

/** A 6000 x 4000 scan: 3:2, landscape. */
const scan: ScanSize = { width: 6000, height: 4000 };
const ratio = (over: Partial<RatioState>): RatioState => ({ ...FREE_RATIO, ...over });
const close = (a: number, b: number) => expect(a).toBeCloseTo(b, 9);

function expectCrop(got: CropRect, want: CropRect) {
  close(got.x, want.x);
  close(got.y, want.y);
  close(got.w, want.w);
  close(got.h, want.h);
}

describe('finishedRatio', () => {
  it('is nothing for no lock', () => {
    expect(finishedRatio(FREE_RATIO, scan, 0)).toBeNull();
  });

  it('gives the preset as width over height, and turned the other way up when portrait', () => {
    expect(finishedRatio(ratio({ mode: '3:2' }), scan, 0)).toBe(1.5);
    expect(finishedRatio(ratio({ mode: '4:3' }), scan, 0)).toBeCloseTo(4 / 3, 12);
    expect(finishedRatio(ratio({ mode: '1:1' }), scan, 0)).toBe(1);
    expect(finishedRatio(ratio({ mode: '16:9' }), scan, 3)).toBeCloseTo(16 / 9, 12);
    expect(finishedRatio(ratio({ mode: '5:4', portrait: true }), scan, 0)).toBeCloseTo(0.8, 12);
  });

  it('gives the scan its own ratio, which turns with the picture', () => {
    expect(finishedRatio(ratio({ mode: 'scan' }), scan, 0)).toBe(1.5);
    expect(finishedRatio(ratio({ mode: 'scan' }), scan, 1)).toBeCloseTo(2 / 3, 12);
    expect(finishedRatio(ratio({ mode: 'scan' }), scan, 2)).toBe(1.5);
    expect(finishedRatio(ratio({ mode: 'scan', portrait: true }), scan, 0)).toBeCloseTo(2 / 3, 12);
  });

  it('reads a custom ratio as typed, with a comma or a point', () => {
    expect(finishedRatio(ratio({ mode: 'custom', custom: { w: '1.85', h: '1' } }), scan, 0)).toBe(
      1.85,
    );
    expect(finishedRatio(ratio({ mode: 'custom', custom: { w: '1,85', h: '1' } }), scan, 0)).toBe(
      1.85,
    );
    expect(
      finishedRatio(ratio({ mode: 'custom', custom: { w: '65', h: '24' } }), scan, 0),
    ).toBeCloseTo(65 / 24, 12);
  });

  it('has no ratio for a custom one that cannot be used', () => {
    for (const custom of [
      { w: '', h: '2' },
      { w: '3', h: ' ' },
      { w: '0', h: '2' },
      { w: '-3', h: '2' },
      { w: 'abc', h: '2' },
      { w: '3', h: '0' },
      { w: '2000', h: '1' },
      { w: '100', h: '1' },
      { w: '1', h: '100' },
      { w: 'Infinity', h: '1' },
    ]) {
      expect(customRatio(custom), JSON.stringify(custom)).toBeNull();
      expect(finishedRatio(ratio({ mode: 'custom', custom }), scan, 0)).toBeNull();
    }
    expect(customRatio({ w: String(MAX_RATIO), h: '1' })).toBe(MAX_RATIO);
    expect(customRatio({ w: '1', h: String(MAX_RATIO) })).toBeCloseTo(1 / MAX_RATIO, 12);
  });
});

describe('from the finished picture to the fractions of the scan', () => {
  it('a quarter turn swaps width and height', () => {
    expect(ratioInScan(1.5, 0)).toBe(1.5);
    expect(ratioInScan(1.5, 1)).toBeCloseTo(2 / 3, 12);
    expect(ratioInScan(1.5, 2)).toBe(1.5);
    expect(ratioInScan(1.5, 3)).toBeCloseTo(2 / 3, 12);
  });

  it('the shape of the scan counts: 3:2 pixels of a 3:2 scan are square fractions', () => {
    expect(fractionRatio(1.5, scan)).toBe(1);
    expect(fractionRatio(1, scan)).toBeCloseTo(2 / 3, 12);
    expect(fractionRatio(1, { width: 4000, height: 6000 })).toBeCloseTo(1.5, 12);
  });

  it('turns a crop back into the ratio of its finished picture', () => {
    expect(ratioOfCrop(FULL_CROP, scan, 0)).toBe(1.5);
    expect(ratioOfCrop(FULL_CROP, scan, 1)).toBeCloseTo(2 / 3, 12);
    expect(ratioOfCrop({ x: 0, y: 0, w: 2 / 3, h: 1 }, scan, 0)).toBeCloseTo(1, 12);
  });
});

describe('fitting the largest crop', () => {
  it('fills the scan when the ratio is the scan’s own', () => {
    expectCrop(fitLargest({ x: 0.3, y: 0.3, w: 0.2, h: 0.2 }, 1), FULL_CROP);
  });

  it('is as wide as the scan allows, and centred on the current crop when there is room', () => {
    // A square 4000 x 4000 crop of a 6000 x 4000 scan: full height, two thirds of the width.
    const k = fractionRatio(1, scan);
    expectCrop(fitLargest({ x: 0.4, y: 0.4, w: 0.2, h: 0.2 }, k), {
      x: 0.5 - k / 2,
      y: 0,
      w: k,
      h: 1,
    });
    // Around a centre near the left edge, it stops at the edge.
    expectCrop(fitLargest({ x: 0, y: 0.4, w: 0.1, h: 0.2 }, k), { x: 0, y: 0, w: k, h: 1 });
    // Around one near the right edge, at the other.
    expectCrop(fitLargest({ x: 0.9, y: 0.4, w: 0.1, h: 0.2 }, k), { x: 1 - k, y: 0, w: k, h: 1 });
  });

  it('is full width and as high as the ratio allows for a wide ratio', () => {
    expectCrop(fitLargest({ x: 0.4, y: 0.4, w: 0.2, h: 0.2 }, 2), { x: 0, y: 0.25, w: 1, h: 0.5 });
    expectCrop(fitLargest({ x: 0.4, y: 0.8, w: 0.2, h: 0.2 }, 2), { x: 0, y: 0.5, w: 1, h: 0.5 });
  });

  it('fits the ratio inside any area, as for the detected frame', () => {
    expectCrop(fitInside({ x: 0.1, y: 0.1, w: 0.5, h: 0.5 }, 2), {
      x: 0.1,
      y: 0.225,
      w: 0.5,
      h: 0.25,
    });
    expectCrop(fitInside({ x: 0.1, y: 0.1, w: 0.5, h: 0.5 }, 0.5), {
      x: 0.225,
      y: 0.1,
      w: 0.25,
      h: 0.5,
    });
  });

  it('a 1:1 picture of a 3:2 scan, turned a quarter, is 2/3 of the width the other way', () => {
    // Finished 3:2 with a quarter turn: 2:3 in the scan, a crop 2667 x 4000 pixels.
    const k = fractionRatio(ratioInScan(1.5, 1), scan);
    const crop = fitLargest(FULL_CROP, k);
    expect(crop.h).toBe(1);
    expect(crop.w * scan.width).toBeCloseTo((2 / 3) * 4000, 6);
    expect(ratioOfCrop(crop, scan, 1)).toBeCloseTo(1.5, 9);
  });
});

describe('resizing with the ratio locked', () => {
  const square: CropRect = { x: 0.2, y: 0.2, w: 0.4, h: 0.4 };

  it('lets a corner grow to cover the pointer, from the opposite corner', () => {
    expectCrop(resizeLocked(square, 'se', { x: 0.8, y: 0.7 }, 1), {
      x: 0.2,
      y: 0.2,
      w: 0.6,
      h: 0.6,
    });
    expectCrop(resizeLocked(square, 'nw', { x: 0.1, y: 0.15 }, 1), {
      x: 0.1,
      y: 0.1,
      w: 0.5,
      h: 0.5,
    });
    expectCrop(resizeLocked(square, 'ne', { x: 0.7, y: 0.1 }, 1), {
      x: 0.2,
      y: 0.1,
      w: 0.5,
      h: 0.5,
    });
    expectCrop(resizeLocked(square, 'sw', { x: 0.1, y: 0.7 }, 1), {
      x: 0.1,
      y: 0.2,
      w: 0.5,
      h: 0.5,
    });
  });

  it('follows a ratio that is not square', () => {
    const wide: CropRect = { x: 0.1, y: 0.1, w: 0.4, h: 0.2 };
    expectCrop(resizeLocked(wide, 'se', { x: 0.9, y: 0.5 }, 2), { x: 0.1, y: 0.1, w: 0.8, h: 0.4 });
  });

  it('stops a corner at the edge of the scan', () => {
    expectCrop(resizeLocked(square, 'se', { x: 2, y: 2 }, 1), { x: 0.2, y: 0.2, w: 0.8, h: 0.8 });
    expectCrop(resizeLocked(square, 'nw', { x: -1, y: -1 }, 1), { x: 0, y: 0, w: 0.6, h: 0.6 });
  });

  it('sets the width from a side and lets the height follow around its centre', () => {
    expectCrop(resizeLocked(square, 'e', { x: 0.8, y: 0 }, 1), { x: 0.2, y: 0.1, w: 0.6, h: 0.6 });
    expectCrop(resizeLocked(square, 'w', { x: 0.1, y: 0 }, 1), { x: 0.1, y: 0.15, w: 0.5, h: 0.5 });
  });

  it('sets the height from a top or bottom side and lets the width follow', () => {
    expectCrop(resizeLocked(square, 'n', { x: 0, y: 0.05 }, 1), {
      x: 0.125,
      y: 0.05,
      w: 0.55,
      h: 0.55,
    });
    expectCrop(resizeLocked(square, 's', { x: 0, y: 0.8 }, 1), { x: 0.1, y: 0.2, w: 0.6, h: 0.6 });
  });

  it('shifts the other dimension rather than leave the scan when the centre is near an edge', () => {
    const low: CropRect = { x: 0.2, y: 0.7, w: 0.2, h: 0.2 };
    expectCrop(resizeLocked(low, 'e', { x: 1, y: 0 }, 1), { x: 0.2, y: 0.2, w: 0.8, h: 0.8 });
  });

  it('does not let a side grow past what the other dimension allows', () => {
    // A tall ratio: the width may not pass k, or the height would pass 1.
    const tall: CropRect = { x: 0.1, y: 0, w: 0.25, h: 1 };
    expectCrop(resizeLocked(tall, 'e', { x: 1, y: 0 }, 0.25), { x: 0.1, y: 0, w: 0.25, h: 1 });
  });

  it('shrinks to the smallest size the recipe allows, and no further', () => {
    const tiny = resizeLocked(square, 'se', { x: 0, y: 0 }, 1);
    expect(tiny.w).toBeCloseTo(MIN_CROP, 12);
    expect(tiny.h).toBeCloseTo(MIN_CROP, 12);
    const tall = resizeLocked({ x: 0.2, y: 0.1, w: 0.1, h: 0.5 }, 'se', { x: 0, y: 0 }, 0.05);
    expect(tall.w).toBeGreaterThanOrEqual(MIN_CROP - 1e-12);
    expect(tall.h).toBeCloseTo(0.2, 12);
  });

  it('keeps the crop inside the scan even with hardly any room next to the anchor', () => {
    const near: CropRect = { x: 0.995, y: 0.5, w: 0.005, h: 0.005 };
    const got = resizeLocked(near, 'se', { x: 1, y: 1 }, 1);
    expect(got.x + got.w).toBeLessThanOrEqual(1 + 1e-12);
    expect(got.y + got.h).toBeLessThanOrEqual(1 + 1e-12);
    expect(got.w).toBeGreaterThanOrEqual(MIN_CROP - 1e-12);
  });

  it('sets a side to a position, as the sliders do', () => {
    expectCrop(moveEdgeLocked(square, 'left', 0.1, 1), { x: 0.1, y: 0.15, w: 0.5, h: 0.5 });
    expectCrop(moveEdgeLocked(square, 'right', 0.8, 1), { x: 0.2, y: 0.1, w: 0.6, h: 0.6 });
    expectCrop(moveEdgeLocked(square, 'top', 0.05, 1), { x: 0.125, y: 0.05, w: 0.55, h: 0.55 });
    expectCrop(moveEdgeLocked(square, 'bottom', 0.8, 1), { x: 0.1, y: 0.2, w: 0.6, h: 0.6 });
  });

  it('always keeps the ratio, the scan and the smallest size, wherever the pointer goes', () => {
    // A small deterministic generator, so a failure can be reproduced.
    let seed = 12345;
    const random = () => {
      seed = (seed * 1664525 + 1013904223) % 4294967296;
      return seed / 4294967296;
    };
    const handles: Handle[] = ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'];
    for (const k of [0.0185, 0.3, 0.667, 1, 1.5, 2.7, 7.4]) {
      for (let i = 0; i < 400; i += 1) {
        const w = MIN_CROP + random() * (1 - MIN_CROP);
        const h = Math.min(1, w / k);
        const start = fitInside(FULL_CROP, k, { x: random(), y: random() });
        const crop = { ...start, w: Math.min(start.w, w), h: Math.min(start.h, h) };
        const fitted = { x: crop.x, y: crop.y, w: crop.h * k, h: crop.h };
        const handle = handles[Math.floor(random() * handles.length)] as Handle;
        const point = { x: random() * 1.4 - 0.2, y: random() * 1.4 - 0.2 };
        const got = resizeLocked(fitted, handle, point, k);
        const label = JSON.stringify({ k, handle, point, fitted, got });
        expect(got.w / got.h, label).toBeCloseTo(k, 6);
        expect(got.x, label).toBeGreaterThanOrEqual(-1e-9);
        expect(got.y, label).toBeGreaterThanOrEqual(-1e-9);
        expect(got.x + got.w, label).toBeLessThanOrEqual(1 + 1e-9);
        expect(got.y + got.h, label).toBeLessThanOrEqual(1 + 1e-9);
        expect(got.w, label).toBeGreaterThanOrEqual(MIN_CROP - 1e-9);
        expect(got.h, label).toBeGreaterThanOrEqual(MIN_CROP - 1e-9);
      }
    }
  });
});
