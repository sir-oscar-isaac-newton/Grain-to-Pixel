import { describe, expect, it } from 'vitest';
import { CLIP_BLACK, CLIP_COLOURS, CLIP_NONE, CLIP_WHITE, buildClipMask, clipClass } from './clip';
import { clipping, computeHistogram } from './histogram';

/** RGBA pixels from [r, g, b] triples, alpha 255. */
const pixels = (...rgb: Array<[number, number, number]>) =>
  Uint8ClampedArray.from(rgb.flatMap(([r, g, b]) => [r, g, b, 255]));

describe('the clipping rule', () => {
  it('is white when any one channel is 255 and black only when all three are 0', () => {
    expect(clipClass(255, 255, 255)).toBe(CLIP_WHITE);
    expect(clipClass(255, 10, 10)).toBe(CLIP_WHITE);
    expect(clipClass(10, 255, 10)).toBe(CLIP_WHITE);
    expect(clipClass(10, 10, 255)).toBe(CLIP_WHITE);
    expect(clipClass(254, 254, 254)).toBe(CLIP_NONE);
    expect(clipClass(0, 0, 0)).toBe(CLIP_BLACK);
    expect(clipClass(0, 10, 10)).toBe(CLIP_NONE);
    expect(clipClass(10, 0, 10)).toBe(CLIP_NONE);
    expect(clipClass(0, 0, 1)).toBe(CLIP_NONE);
    expect(clipClass(128, 128, 128)).toBe(CLIP_NONE);
    // A pixel with a 255 and zeros elsewhere is a highlight, not a shadow.
    expect(clipClass(255, 0, 0)).toBe(CLIP_WHITE);
  });

  it('builds a mask: red for white-clipped pixels, blue for black-clipped, clear elsewhere', () => {
    const rgba = pixels(
      [255, 255, 255], // white
      [0, 200, 255], // one channel at 255
      [254, 254, 254], // just short
      [0, 0, 0], // black
      [0, 0, 5], // one channel at 0 only
      [128, 128, 128], // mid grey
    );
    const mask = buildClipMask(rgba);
    expect(mask.white).toBe(2);
    expect(mask.black).toBe(1);
    expect(mask.rgba).toHaveLength(rgba.length);
    const at = (i: number) => [...mask.rgba.slice(i * 4, i * 4 + 4)];
    const red = [...CLIP_COLOURS.white, 255];
    const blue = [...CLIP_COLOURS.black, 255];
    expect([0, 1, 2, 3, 4, 5].map(at)).toEqual([
      red,
      red,
      [0, 0, 0, 0],
      blue,
      [0, 0, 0, 0],
      [0, 0, 0, 0],
    ]);
  });

  it('ignores alpha and copes with an empty buffer', () => {
    expect(buildClipMask(Uint8ClampedArray.from([255, 0, 0, 0])).white).toBe(1);
    expect(buildClipMask(new Uint8ClampedArray(0))).toEqual({
      rgba: new Uint8ClampedArray(0),
      white: 0,
      black: 0,
    });
  });

  it('gives the caption the same counts as the mask', () => {
    const rgba = pixels(
      [255, 255, 255],
      [255, 10, 20],
      [0, 0, 0],
      [0, 0, 7],
      [90, 90, 90],
      [10, 20, 30],
      [0, 0, 0],
      [200, 255, 0],
    );
    const mask = buildClipMask(rgba);
    const bins = computeHistogram(rgba);
    expect([bins.whiteClipped, bins.blackClipped]).toEqual([mask.white, mask.black]);
    expect(clipping(bins)).toEqual({ white: mask.white / 8, black: mask.black / 8 });
    expect(clipping(bins)).toEqual({ white: 3 / 8, black: 2 / 8 });
  });
});
