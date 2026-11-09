// Focus aids for the live view, on the frame's brightness: the edges to outline (focus peaking)
// and a sharpness number. Pure functions on typed arrays, so they are tested without a canvas.

/** A region of an image in pixels; `x1` and `y1` are exclusive. */
export interface Rect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** The colours peaking can outline in, as red, green and blue (0 to 255). */
export const PEAK_COLOURS = {
  red: [255, 40, 40],
  green: [40, 255, 40],
  yellow: [255, 235, 40],
  white: [255, 255, 255],
} as const;

export type PeakColour = keyof typeof PEAK_COLOURS;

/** The sensitivity control runs from 1 (only the strongest edges) to 100 (nearly every edge). */
export const MIN_SENSITIVITY = 1;
export const MAX_SENSITIVITY = 100;

/**
 * The Sobel magnitude (on brightness 0 to 255) above which a pixel is an edge: from 320 at
 * sensitivity 1 down to 20 at 100.
 */
export function thresholdFor(sensitivity: number): number {
  const s = Math.min(MAX_SENSITIVITY, Math.max(MIN_SENSITIVITY, sensitivity));
  return 320 - ((s - MIN_SENSITIVITY) / (MAX_SENSITIVITY - MIN_SENSITIVITY)) * 300;
}

/** Brightness (Rec. 601 weights, 0 to 255) of every pixel of an RGBA buffer, into `out`. */
export function luminanceOf(rgba: Uint8ClampedArray, out: Float32Array): void {
  for (let i = 0, p = 0; p < out.length; i += 4, p += 1) {
    out[p] = 0.299 * (rgba[i] ?? 0) + 0.587 * (rgba[i + 1] ?? 0) + 0.114 * (rgba[i + 2] ?? 0);
  }
}

/**
 * Marks the pixels whose Sobel magnitude exceeds `threshold` with 1 in `mask` (others 0; the
 * one-pixel border is 0) and returns how many were marked.
 */
export function edgeMask(
  lum: Float32Array,
  width: number,
  height: number,
  threshold: number,
  mask: Uint8Array,
): number {
  mask.fill(0);
  const limit = threshold * threshold;
  let count = 0;
  for (let y = 1; y < height - 1; y += 1) {
    const row = y * width;
    for (let x = 1; x < width - 1; x += 1) {
      const i = row + x;
      const tl = lum[i - width - 1] ?? 0;
      const tc = lum[i - width] ?? 0;
      const tr = lum[i - width + 1] ?? 0;
      const ml = lum[i - 1] ?? 0;
      const mr = lum[i + 1] ?? 0;
      const bl = lum[i + width - 1] ?? 0;
      const bc = lum[i + width] ?? 0;
      const br = lum[i + width + 1] ?? 0;
      const gx = tr + 2 * mr + br - tl - 2 * ml - bl;
      const gy = bl + 2 * bc + br - tl - 2 * tc - tr;
      if (gx * gx + gy * gy > limit) {
        mask[i] = 1;
        count += 1;
      }
    }
  }
  return count;
}

/** Fills an RGBA buffer: the colour, opaque, where `mask` is set, and transparent elsewhere. */
export function paintMask(
  mask: Uint8Array,
  rgba: Uint8ClampedArray,
  [r, g, b]: readonly [number, number, number],
): void {
  for (let p = 0, i = 0; p < mask.length; p += 1, i += 4) {
    rgba[i] = r;
    rgba[i + 1] = g;
    rgba[i + 2] = b;
    rgba[i + 3] = mask[p] === 1 ? 255 : 0;
  }
}

/** The part of the frame the sharpness is read from: its central third, or all of it when zoomed. */
export function sharpnessRegion(width: number, height: number, zoomed: boolean): Rect {
  if (zoomed) return { x0: 0, y0: 0, x1: width, y1: height };
  return {
    x0: Math.floor(width / 3),
    y0: Math.floor(height / 3),
    x1: Math.floor((2 * width) / 3),
    y1: Math.floor((2 * height) / 3),
  };
}

/**
 * The sharpness number: the variance of the Laplacian of the brightness over `region` (raw, so
 * 0 for a flat picture and larger for a sharper one). Pixels on the image border are skipped.
 */
export function laplacianVariance(
  lum: Float32Array,
  width: number,
  height: number,
  region: Rect,
): number {
  const x0 = Math.max(1, region.x0);
  const y0 = Math.max(1, region.y0);
  const x1 = Math.min(width - 1, region.x1);
  const y1 = Math.min(height - 1, region.y1);
  let sum = 0;
  let sumSquares = 0;
  let n = 0;
  for (let y = y0; y < y1; y += 1) {
    for (let x = x0; x < x1; x += 1) {
      const i = y * width + x;
      const l =
        (lum[i - width] ?? 0) +
        (lum[i + width] ?? 0) +
        (lum[i - 1] ?? 0) +
        (lum[i + 1] ?? 0) -
        4 * (lum[i] ?? 0);
      sum += l;
      sumSquares += l * l;
      n += 1;
    }
  }
  if (n === 0) return 0;
  const mean = sum / n;
  return Math.max(0, sumSquares / n - mean * mean);
}

/**
 * The share of the whole frame that the camera's zoom shows, from the step's label ("25 %",
 * "200 %"): the zoom factor is about 0.094 per percent (100 % is about 640 of 6016 sensor
 * pixels, so about 9.4 times). 1 for a label without a number (the whole frame).
 */
export function visibleFraction(label: string): number {
  const percent = Number.parseFloat(label);
  if (!Number.isFinite(percent) || percent <= 0) return 1;
  return Math.min(1, 1 / (percent * 0.094));
}

/** One nudge of the zoomed area: a quarter of what is visible, or 0.1 at the whole frame. */
export function nudgeStep(label: string): number {
  const visible = visibleFraction(label);
  return visible >= 1 ? 0.1 : 0.25 * visible;
}

/** A point moved by `dx`, `dy` (in fractions), kept inside the frame. */
export function moved(
  point: { x: number; y: number },
  dx: number,
  dy: number,
): { x: number; y: number } {
  const clamp = (v: number) => Math.min(1, Math.max(0, v));
  return { x: clamp(point.x + dx), y: clamp(point.y + dy) };
}
