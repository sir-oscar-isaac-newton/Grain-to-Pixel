/**
 * The clipping rule of the Develop view, in one place: the overlay and the percentages under the
 * histogram both come from it, so they always agree.
 *
 * A pixel of the 8-bit preview is clipped to white when any one of its R, G, B values is 255, and
 * clipped to black when all three are 0. Alpha is ignored.
 */
export const CLIP_NONE = 0;
export const CLIP_WHITE = 1;
export const CLIP_BLACK = 2;
export type ClipClass = typeof CLIP_NONE | typeof CLIP_WHITE | typeof CLIP_BLACK;

/** Which way a pixel is clipped, if at all. */
export function clipClass(r: number, g: number, b: number): ClipClass {
  if (r === 255 || g === 255 || b === 255) return CLIP_WHITE;
  if (r === 0 && g === 0 && b === 0) return CLIP_BLACK;
  return CLIP_NONE;
}

/** The overlay colours as [r, g, b]: red for clipped highlights, blue for clipped shadows. */
export const CLIP_COLOURS = {
  white: [255, 45, 45],
  black: [60, 120, 255],
} as const;

/** The overlay of an RGBA8 buffer: RGBA pixels, opaque where clipped and transparent elsewhere. */
export interface ClipMask {
  rgba: Uint8ClampedArray;
  /** Pixels clipped to white. */
  white: number;
  /** Pixels clipped to black. */
  black: number;
}

/** One pass over the pixels: the mask of the clipped ones and how many of each kind there are. */
export function buildClipMask(rgba: ArrayLike<number>): ClipMask {
  const pixels = Math.floor(rgba.length / 4);
  const out = new Uint8ClampedArray(pixels * 4);
  const [wr, wg, wb] = CLIP_COLOURS.white;
  const [br, bg, bb] = CLIP_COLOURS.black;
  let white = 0;
  let black = 0;
  for (let i = 0; i < pixels; i += 1) {
    const kind = clipClass(rgba[i * 4] ?? 0, rgba[i * 4 + 1] ?? 0, rgba[i * 4 + 2] ?? 0);
    if (kind === CLIP_NONE) continue;
    const at = i * 4;
    if (kind === CLIP_WHITE) {
      white += 1;
      out[at] = wr;
      out[at + 1] = wg;
      out[at + 2] = wb;
    } else {
      black += 1;
      out[at] = br;
      out[at + 1] = bg;
      out[at + 2] = bb;
    }
    out[at + 3] = 255;
  }
  return { rgba: out, white, black };
}
