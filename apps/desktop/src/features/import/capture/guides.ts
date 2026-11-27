// The geometry of the live view's frame lines (U-38). Everything is in fractions of the picture
// (0 to 1 across and down), so the lines stay on the picture at any window size. Pure functions.

import type { FilmFormat } from '../../../shared/ipc';

/** A rectangle on the picture, as fractions of it; `x` and `y` are the top left. */
export interface GuideRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type Orientation = 'landscape' | 'portrait';

/** The long side over the short side of one frame per film format (as `FilmFormat::aspect`). */
export const FORMAT_ASPECT: Record<FilmFormat, number> = {
  '35mm': 36 / 24,
  '645': 56 / 41.5,
  '6x6': 1,
  '6x7': 70 / 56,
  '6x9': 84 / 56,
};

/** The default lines fit in this share of the picture. */
export const FIT = 0.9;
/** The smallest side of the lines, as a fraction of the picture. */
export const MIN_SIDE = 0.05;
/** One arrow key press moves or resizes by this fraction of the picture. */
export const KEY_STEP = 0.005;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The key under which adjusted lines are kept: `35mm-landscape`. */
export const guideKey = (format: FilmFormat, orientation: Orientation) =>
  `${format}-${orientation}`;

/**
 * The lines before any adjusting. `ratio` is the long side over the short side of one frame (from
 * a stored frame size or the film format). `pictureAspect` is the picture's width over its height
 * in pixels, so the rectangle has the right shape on the picture and not just in fractions.
 * A stored frame size (fractions of a capture) is used at its own size, centred; the format's
 * ratio gives the largest rectangle that fits in 90 % of the picture.
 */
export function defaultRect(options: {
  format: FilmFormat;
  frameSize: { w: number | null; h: number | null } | null;
  orientation: Orientation;
  pictureAspect: number;
}): GuideRect {
  const { format, frameSize, orientation, pictureAspect } = options;
  const aspect = Number.isFinite(pictureAspect) && pictureAspect > 0 ? pictureAspect : 1.5;
  const stored =
    frameSize !== null &&
    frameSize.w !== null &&
    frameSize.h !== null &&
    Number.isFinite(frameSize.w) &&
    Number.isFinite(frameSize.h) &&
    frameSize.w > 0 &&
    frameSize.h > 0
      ? { w: Math.min(1, frameSize.w), h: Math.min(1, frameSize.h) }
      : null;
  let w: number;
  let h: number;
  if (stored) {
    // The frame in pixel terms (width unit is the picture's height): turn it, then back to fractions.
    const pw = stored.w * aspect;
    const ph = stored.h;
    const long = Math.max(pw, ph);
    const short = Math.min(pw, ph);
    const [tw, th] = orientation === 'landscape' ? [long, short] : [short, long];
    w = tw / aspect;
    h = th;
    // A turned frame that would stick out is scaled down to fit.
    const scale = Math.min(1, 1 / w, 1 / h);
    w *= scale;
    h *= scale;
  } else {
    const ratio = FORMAT_ASPECT[format] ?? 1.5;
    const target = orientation === 'landscape' ? ratio : 1 / ratio;
    // Pixel ratio of the rectangle is (w * aspect) / h = target.
    w = Math.min(FIT, (FIT * target) / aspect);
    h = (w * aspect) / target;
  }
  return { x: (1 - w) / 2, y: (1 - h) / 2, w, h };
}

/** The rectangle kept inside the picture and at least [`MIN_SIDE`] on each side. */
export function clampRect(rect: GuideRect): GuideRect {
  const w = clamp(Number.isFinite(rect.w) ? rect.w : MIN_SIDE, MIN_SIDE, 1);
  const h = clamp(Number.isFinite(rect.h) ? rect.h : MIN_SIDE, MIN_SIDE, 1);
  const x = clamp(Number.isFinite(rect.x) ? rect.x : 0, 0, 1 - w);
  const y = clamp(Number.isFinite(rect.y) ? rect.y : 0, 0, 1 - h);
  return { x, y, w, h };
}

/** The rectangle moved by a distance in fractions of the picture, and kept inside it. */
export function moveRect(rect: GuideRect, dx: number, dy: number): GuideRect {
  return clampRect({ ...rect, x: rect.x + dx, y: rect.y + dy });
}

export type Corner = 'nw' | 'ne' | 'sw' | 'se';

/** The rectangle with one corner dragged to a point; the opposite corner stays where it is. */
export function dragCorner(rect: GuideRect, corner: Corner, point: { x: number; y: number }) {
  const fixedX = corner === 'nw' || corner === 'sw' ? rect.x + rect.w : rect.x;
  const fixedY = corner === 'nw' || corner === 'ne' ? rect.y + rect.h : rect.y;
  const leftOfFixed = corner === 'nw' || corner === 'sw';
  const aboveFixed = corner === 'nw' || corner === 'ne';
  const px = leftOfFixed
    ? clamp(point.x, 0, fixedX - MIN_SIDE)
    : clamp(point.x, fixedX + MIN_SIDE, 1);
  const py = aboveFixed
    ? clamp(point.y, 0, fixedY - MIN_SIDE)
    : clamp(point.y, fixedY + MIN_SIDE, 1);
  return clampRect({
    x: Math.min(px, fixedX),
    y: Math.min(py, fixedY),
    w: Math.abs(px - fixedX),
    h: Math.abs(py - fixedY),
  });
}

/** What an arrow key does to the lines: it moves them; with Shift it resizes about the centre. */
export function keyRect(rect: GuideRect, key: string, shift: boolean): GuideRect | null {
  const dx = key === 'ArrowLeft' ? -1 : key === 'ArrowRight' ? 1 : 0;
  const dy = key === 'ArrowUp' ? -1 : key === 'ArrowDown' ? 1 : 0;
  if (dx === 0 && dy === 0) return null;
  if (!shift) return moveRect(rect, dx * KEY_STEP, dy * KEY_STEP);
  // Right and Up grow, Left and Down shrink (width by Left/Right, height by Up/Down).
  const dw = dx * KEY_STEP * 2;
  const dh = -dy * KEY_STEP * 2;
  const w = clamp(rect.w + dw, MIN_SIDE, 1);
  const h = clamp(rect.h + dh, MIN_SIDE, 1);
  return clampRect({
    x: rect.x + (rect.w - w) / 2,
    y: rect.y + (rect.h - h) / 2,
    w,
    h,
  });
}

/** The positions (fractions) of the grid's inner lines: thirds or a fine grid of tenths. */
export function gridLines(kind: 'thirds' | 'fine'): number[] {
  const n = kind === 'thirds' ? 3 : 10;
  return Array.from({ length: n - 1 }, (_, i) => (i + 1) / n);
}

/** The same two rectangles are the same lines (to within a rounding error). */
export function sameRect(a: GuideRect, b: GuideRect): boolean {
  return [a.x - b.x, a.y - b.y, a.w - b.w, a.h - b.h].every((d) => Math.abs(d) < 1e-6);
}

/** A stored rectangle read back: finite numbers inside the picture, else null. */
export function readRect(rect: {
  x: number | null;
  y: number | null;
  w: number | null;
  h: number | null;
}): GuideRect | null {
  const { x, y, w, h } = rect;
  if (x === null || y === null || w === null || h === null) return null;
  if (![x, y, w, h].every(Number.isFinite) || w <= 0 || h <= 0) return null;
  return clampRect({ x, y, w, h });
}
