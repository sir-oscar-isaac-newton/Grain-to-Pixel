import { describe, expect, it } from 'vitest';
import {
  edgeMask,
  laplacianVariance,
  luminanceOf,
  moved,
  nudgeStep,
  paintMask,
  sharpnessRegion,
  thresholdFor,
  visibleFraction,
} from './focus';

const W = 48;
const H = 32;

/** A checkerboard of 4 pixel squares, 0 and 255. */
function checkerboard(): Float32Array {
  const lum = new Float32Array(W * H);
  for (let y = 0; y < H; y += 1) {
    for (let x = 0; x < W; x += 1) {
      lum[y * W + x] = (Math.floor(x / 4) + Math.floor(y / 4)) % 2 === 0 ? 0 : 255;
    }
  }
  return lum;
}

/** A box blur, `passes` times. */
function blurred(source: Float32Array, passes: number): Float32Array {
  let current = source;
  for (let pass = 0; pass < passes; pass += 1) {
    const next = new Float32Array(current.length);
    for (let y = 0; y < H; y += 1) {
      for (let x = 0; x < W; x += 1) {
        let sum = 0;
        let n = 0;
        for (let dy = -1; dy <= 1; dy += 1) {
          for (let dx = -1; dx <= 1; dx += 1) {
            const xx = x + dx;
            const yy = y + dy;
            if (xx >= 0 && xx < W && yy >= 0 && yy < H) {
              sum += current[yy * W + xx] ?? 0;
              n += 1;
            }
          }
        }
        next[y * W + x] = sum / n;
      }
    }
    current = next;
  }
  return current;
}

/** Flat grey with one vertical edge in the middle. */
function oneEdge(): Float32Array {
  const lum = new Float32Array(W * H);
  for (let y = 0; y < H; y += 1) {
    for (let x = 0; x < W; x += 1) lum[y * W + x] = x < W / 2 ? 40 : 200;
  }
  return lum;
}

describe('sharpness', () => {
  const region = sharpnessRegion(W, H, true);

  it('is higher for a sharp picture than for a blurred one, and falls as the blur grows', () => {
    const sharp = laplacianVariance(checkerboard(), W, H, region);
    const soft = laplacianVariance(blurred(checkerboard(), 1), W, H, region);
    const softer = laplacianVariance(blurred(checkerboard(), 3), W, H, region);
    expect(sharp).toBeGreaterThan(soft);
    expect(soft).toBeGreaterThan(softer);
  });

  it('is 0 for a flat picture', () => {
    expect(laplacianVariance(new Float32Array(W * H).fill(90), W, H, region)).toBe(0);
  });

  it('reads the central third at the whole frame and everything when zoomed', () => {
    expect(sharpnessRegion(90, 60, false)).toEqual({ x0: 30, y0: 20, x1: 60, y1: 40 });
    expect(sharpnessRegion(90, 60, true)).toEqual({ x0: 0, y0: 0, x1: 90, y1: 60 });
    // Detail only outside the middle does not count at the whole frame.
    const lum = new Float32Array(W * H).fill(100);
    for (let y = 0; y < 6; y += 1) for (let x = 0; x < W; x += 2) lum[y * W + x] = 255;
    expect(laplacianVariance(lum, W, H, sharpnessRegion(W, H, false))).toBe(0);
    expect(laplacianVariance(lum, W, H, sharpnessRegion(W, H, true))).toBeGreaterThan(0);
  });

  it('turns RGBA into brightness', () => {
    const out = new Float32Array(2);
    luminanceOf(new Uint8ClampedArray([255, 255, 255, 255, 0, 0, 0, 255]), out);
    expect(out[0]).toBeCloseTo(255, 3);
    expect(out[1]).toBe(0);
  });
});

describe('edge mask', () => {
  it('marks the pixels at an edge and none in flat areas', () => {
    const mask = new Uint8Array(W * H);
    const count = edgeMask(oneEdge(), W, H, thresholdFor(50), mask);
    expect(count).toBeGreaterThan(0);
    expect(mask[10 * W + W / 2]).toBe(1);
    expect(mask[10 * W + W / 2 - 1]).toBe(1);
    expect(mask[10 * W + 5]).toBe(0);
    expect(mask[10 * W + W - 5]).toBe(0);
    expect(edgeMask(new Float32Array(W * H).fill(90), W, H, thresholdFor(100), mask)).toBe(0);
  });

  it('marks more pixels at a lower threshold, which is what a higher sensitivity sets', () => {
    // Edges of every strength: a ramp that gets steeper from row to row.
    const lum = new Float32Array(W * H);
    for (let y = 0; y < H; y += 1) {
      for (let x = 0; x < W; x += 1) lum[y * W + x] = Math.min(255, x * (y + 1) * 0.4);
    }
    const mask = new Uint8Array(W * H);
    const strict = edgeMask(lum, W, H, 100, mask);
    const middle = edgeMask(lum, W, H, 50, mask);
    const loose = edgeMask(lum, W, H, 20, mask);
    expect(strict).toBeLessThan(middle);
    expect(middle).toBeLessThan(loose);
    expect(thresholdFor(95)).toBeLessThan(thresholdFor(10));
  });

  it('never marks the border', () => {
    const mask = new Uint8Array(W * H);
    edgeMask(checkerboard(), W, H, 1, mask);
    expect(mask[0]).toBe(0);
    expect(mask[W - 1]).toBe(0);
    expect(mask[(H - 1) * W]).toBe(0);
  });

  it('paints the colour where marked and leaves the rest transparent', () => {
    const mask = new Uint8Array([1, 0]);
    const rgba = new Uint8ClampedArray(8);
    paintMask(mask, rgba, [10, 20, 30]);
    expect([...rgba]).toEqual([10, 20, 30, 255, 10, 20, 30, 0]);
  });

  it('takes a threshold that falls as the sensitivity rises', () => {
    expect(thresholdFor(1)).toBeGreaterThan(thresholdFor(50));
    expect(thresholdFor(50)).toBeGreaterThan(thresholdFor(100));
    expect(thresholdFor(-5)).toBe(thresholdFor(1));
  });
});

describe('zoomed area', () => {
  it('knows how much of the frame a step shows', () => {
    expect(visibleFraction('Whole frame')).toBe(1);
    expect(visibleFraction('100 %')).toBeCloseTo(640 / 6016, 2);
    expect(visibleFraction('200 %')).toBeLessThan(visibleFraction('100 %'));
    expect(visibleFraction('25 %')).toBeGreaterThan(visibleFraction('50 %'));
  });

  it('nudges by a quarter of the view, or a tenth at the whole frame', () => {
    expect(nudgeStep('Whole frame')).toBe(0.1);
    expect(nudgeStep('100 %')).toBeCloseTo(0.25 * visibleFraction('100 %'), 6);
  });

  it('keeps the point inside the frame', () => {
    expect(moved({ x: 0.95, y: 0.02 }, 0.1, -0.1)).toEqual({ x: 1, y: 0 });
    expect(moved({ x: 0.5, y: 0.5 }, 0.1, 0.2)).toEqual({ x: 0.6, y: 0.7 });
  });
});
