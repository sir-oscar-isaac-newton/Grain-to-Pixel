import { describe, expect, it } from 'vitest';
import { clipping, computeHistogram } from './histogram';

/** RGBA pixels from [r, g, b] triples, alpha 255. */
const pixels = (...rgb: Array<[number, number, number]>) =>
  Uint8ClampedArray.from(rgb.flatMap(([r, g, b]) => [r, g, b, 255]));

describe('histogram', () => {
  it('counts each channel and the Rec.709 luma, ignoring alpha', () => {
    const bins = computeHistogram(
      Uint8ClampedArray.from([10, 20, 30, 0, 10, 20, 30, 255, 255, 255, 255, 255]),
    );
    expect(bins.pixels).toBe(3);
    expect([bins.red[10], bins.red[255]]).toEqual([2, 1]);
    expect([bins.green[20], bins.green[255]]).toEqual([2, 1]);
    expect([bins.blue[30], bins.blue[255]]).toEqual([2, 1]);
    // 0.2126 * 10 + 0.7152 * 20 + 0.0722 * 30 = 18.6, which rounds to 19.
    expect(bins.luma[19]).toBe(2);
    expect(bins.luma[255]).toBe(1);
    for (const channel of [bins.red, bins.green, bins.blue, bins.luma]) {
      expect(channel).toHaveLength(256);
      expect([...channel].reduce((a, b) => a + b, 0)).toBe(3);
    }
  });

  it('reports the share of pixels lost to pure black and pure white', () => {
    const bins = computeHistogram(pixels([0, 0, 0], [0, 0, 0], [255, 255, 255], [90, 90, 90]));
    expect(clipping(bins)).toEqual({ black: 0.5, white: 0.25 });
    // A pixel that is black in one channel only is not lost.
    expect(clipping(computeHistogram(pixels([0, 120, 120])))).toEqual({ black: 0, white: 0 });
  });

  it('copes with an empty picture', () => {
    const bins = computeHistogram(new Uint8ClampedArray(0));
    expect(bins.pixels).toBe(0);
    expect(clipping(bins)).toEqual({ black: 0, white: 0 });
  });
});
