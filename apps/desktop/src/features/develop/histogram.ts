import { CLIP_BLACK, CLIP_WHITE, clipClass } from './clip';

/** 256-bin histograms of 8-bit display values, as the Develop view shows them. */
export interface Bins {
  red: Uint32Array;
  green: Uint32Array;
  blue: Uint32Array;
  /** Rec.709 luma of the display values. */
  luma: Uint32Array;
  /** The pixels counted. */
  pixels: number;
  /** Pixels clipped to white and to black by the rule of `clip.ts`. */
  whiteClipped: number;
  blackClipped: number;
}

/** Histograms of an RGBA8 buffer (alpha is ignored). */
export function computeHistogram(rgba: ArrayLike<number>): Bins {
  const red = new Uint32Array(256);
  const green = new Uint32Array(256);
  const blue = new Uint32Array(256);
  const luma = new Uint32Array(256);
  const pixels = Math.floor(rgba.length / 4);
  let whiteClipped = 0;
  let blackClipped = 0;
  for (let i = 0; i < pixels; i += 1) {
    const r = rgba[i * 4] ?? 0;
    const g = rgba[i * 4 + 1] ?? 0;
    const b = rgba[i * 4 + 2] ?? 0;
    red[r] = (red[r] ?? 0) + 1;
    green[g] = (green[g] ?? 0) + 1;
    blue[b] = (blue[b] ?? 0) + 1;
    const y = Math.round(0.2126 * r + 0.7152 * g + 0.0722 * b);
    luma[y] = (luma[y] ?? 0) + 1;
    const kind = clipClass(r, g, b);
    if (kind === CLIP_WHITE) whiteClipped += 1;
    else if (kind === CLIP_BLACK) blackClipped += 1;
  }
  return { red, green, blue, luma, pixels, whiteClipped, blackClipped };
}

/** The share of pixels (0 to 1) clipped to black and to white, by the rule of `clip.ts`. */
export function clipping(bins: Bins): { black: number; white: number } {
  if (bins.pixels === 0) return { black: 0, white: 0 };
  return { black: bins.blackClipped / bins.pixels, white: bins.whiteClipped / bins.pixels };
}
