/** 256-bin histograms of 8-bit display values, as the Develop view shows them. */
export interface Bins {
  red: Uint32Array;
  green: Uint32Array;
  blue: Uint32Array;
  /** Rec.709 luma of the display values. */
  luma: Uint32Array;
  /** The pixels counted. */
  pixels: number;
}

/** Histograms of an RGBA8 buffer (alpha is ignored). */
export function computeHistogram(rgba: ArrayLike<number>): Bins {
  const red = new Uint32Array(256);
  const green = new Uint32Array(256);
  const blue = new Uint32Array(256);
  const luma = new Uint32Array(256);
  const pixels = Math.floor(rgba.length / 4);
  for (let i = 0; i < pixels; i += 1) {
    const r = rgba[i * 4] ?? 0;
    const g = rgba[i * 4 + 1] ?? 0;
    const b = rgba[i * 4 + 2] ?? 0;
    red[r] = (red[r] ?? 0) + 1;
    green[g] = (green[g] ?? 0) + 1;
    blue[b] = (blue[b] ?? 0) + 1;
    const y = Math.round(0.2126 * r + 0.7152 * g + 0.0722 * b);
    luma[y] = (luma[y] ?? 0) + 1;
  }
  return { red, green, blue, luma, pixels };
}

/** The share of pixels (0 to 1) whose luma is pure black and pure white: lost shadows and highlights. */
export function clipping(bins: Bins): { black: number; white: number } {
  if (bins.pixels === 0) return { black: 0, white: 0 };
  return {
    black: (bins.luma[0] ?? 0) / bins.pixels,
    white: (bins.luma[255] ?? 0) / bins.pixels,
  };
}
