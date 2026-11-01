// Locking the crop to an aspect ratio. The ratio is that of the finished picture, after the quarter
// turns; the crop is stored in the scan's own orientation, as fractions of the scan, so the maths
// here goes from the finished picture's ratio to the fractions that give it:
//
//   finished ratio  -- a quarter turn swaps width and height -->  ratio in the scan
//   ratio in the scan  -- times scan height over scan width -->  width over height in fractions
//
// Straightening keeps the crop's ratio (the geometry stage cuts the largest rectangle of the crop's
// own ratio out of the rotated crop), so it needs no handling here.

import { MIN_CROP, clamp, type CropRect, type Edge, type Handle } from './crop';

/** The size of the scan in pixels (the proportions are what matter). */
export interface ScanSize {
  width: number;
  height: number;
}

/** What the lock is set to. `free` is no lock; `scan` is the scan's own ratio. */
export type RatioMode = 'free' | 'scan' | '3:2' | '4:3' | '5:4' | '1:1' | '16:9' | 'custom';

export const PRESET_MODES = ['scan', '3:2', '4:3', '5:4', '1:1', '16:9'] as const;

export interface RatioState {
  mode: RatioMode;
  /** The custom ratio as typed, width and height (kept as text so it can be edited). */
  custom: { w: string; h: string };
  /** Portrait instead of landscape: the ratio turned the other way up. */
  portrait: boolean;
}

export const FREE_RATIO: RatioState = {
  mode: 'free',
  custom: { w: '3', h: '2' },
  portrait: false,
};

/** A custom ratio may be at most this much wider than tall, or taller than wide. */
export const MAX_RATIO = 20;

/** The number typed, or null when it is not a positive finite number. */
function parse(text: string): number | null {
  const value = Number(text.trim().replace(',', '.'));
  return text.trim() !== '' && Number.isFinite(value) && value > 0 && value <= 1000 ? value : null;
}

/** The custom ratio's width over height, or null when it is not a usable ratio. */
export function customRatio(custom: { w: string; h: string }): number | null {
  const w = parse(custom.w);
  const h = parse(custom.h);
  if (w === null || h === null) return null;
  const ratio = w / h;
  return ratio >= 1 / MAX_RATIO && ratio <= MAX_RATIO ? ratio : null;
}

/**
 * The finished picture's width over height for a ratio setting, or null when there is no lock (or
 * a custom ratio that cannot be used). The scan's own ratio turns with the picture.
 */
export function finishedRatio(state: RatioState, scan: ScanSize, rot90: number): number | null {
  let ratio: number | null;
  switch (state.mode) {
    case 'free':
      return null;
    case 'scan':
      ratio = rot90 % 2 === 0 ? scan.width / scan.height : scan.height / scan.width;
      break;
    case 'custom':
      ratio = customRatio(state.custom);
      break;
    default: {
      const [w, h] = state.mode.split(':').map(Number);
      ratio = (w ?? 1) / (h ?? 1);
    }
  }
  if (ratio === null) return null;
  return state.portrait ? 1 / ratio : ratio;
}

/** The ratio of the crop in the scan's own orientation, for a finished picture's ratio. */
export function ratioInScan(finished: number, rot90: number): number {
  return rot90 % 2 === 0 ? finished : 1 / finished;
}

/** Width over height of the crop as fractions of the scan, for a ratio in the scan's own pixels. */
export function fractionRatio(inScan: number, scan: ScanSize): number {
  return (inScan * scan.height) / scan.width;
}

/** The ratio of a crop's finished picture (width over height), the other way round. */
export function ratioOfCrop(crop: CropRect, scan: ScanSize, rot90: number): number {
  const inScan = (crop.w * scan.width) / (crop.h * scan.height);
  return rot90 % 2 === 0 ? inScan : 1 / inScan;
}

/**
 * The largest crop of a ratio (`k`: width over height, in fractions of the scan) that fits inside
 * `area`, as close as it can be to being centred on `around` (the centre of the area by default).
 */
export function fitInside(
  area: CropRect,
  k: number,
  around: { x: number; y: number } = { x: area.x + area.w / 2, y: area.y + area.h / 2 },
): CropRect {
  const h = Math.min(area.h, area.w / k);
  const w = k * h;
  return {
    x: clamp(around.x - w / 2, area.x, area.x + area.w - w),
    y: clamp(around.y - h / 2, area.y, area.y + area.h - h),
    w,
    h,
  };
}

/** The largest crop of ratio `k` around the centre of the current crop, inside the scan. */
export function fitLargest(crop: CropRect, k: number): CropRect {
  return fitInside({ x: 0, y: 0, w: 1, h: 1 }, k, {
    x: crop.x + crop.w / 2,
    y: crop.y + crop.h / 2,
  });
}

/** The room the crop can grow into from a fixed anchor, towards `sign` (+1 right or down). */
const room = (anchor: number, sign: number) => (sign > 0 ? 1 - anchor : anchor);

/** The crop moved, if need be, to lie inside the scan (only when there is hardly any room left). */
const inside = (r: CropRect): CropRect => ({
  ...r,
  x: clamp(r.x, 0, 1 - r.w),
  y: clamp(r.y, 0, 1 - r.h),
});

/** The smallest height a crop of ratio `k` can have: neither side under the recipe's minimum. */
const minHeight = (k: number) => Math.max(MIN_CROP, MIN_CROP / k);

/**
 * The crop with a handle pulled to `point` (fractions of the scan) while keeping ratio `k`. A
 * corner keeps the opposite corner where it is and grows to cover the pointer; a side sets that
 * dimension and the other follows around its centre. The crop stays inside the scan.
 */
export function resizeLocked(
  crop: CropRect,
  handle: Handle,
  point: { x: number; y: number },
  k: number,
): CropRect {
  const sx = handle.includes('e') ? 1 : handle.includes('w') ? -1 : 0;
  const sy = handle.includes('s') ? 1 : handle.includes('n') ? -1 : 0;
  const ax = sx > 0 ? crop.x : crop.x + crop.w;
  const ay = sy > 0 ? crop.y : crop.y + crop.h;
  const cx = crop.x + crop.w / 2;
  const cy = crop.y + crop.h / 2;

  if (sx !== 0 && sy !== 0) {
    const reachW = Math.max(0, (point.x - ax) * sx);
    const reachH = Math.max(0, (point.y - ay) * sy);
    const most = Math.min(room(ay, sy), room(ax, sx) / k);
    const h = clamp(Math.max(reachH, reachW / k), minHeight(k), Math.max(most, minHeight(k)));
    const w = k * h;
    return inside({ x: sx > 0 ? ax : ax - w, y: sy > 0 ? ay : ay - h, w, h });
  }
  if (sx !== 0) {
    // A side: the width is set, the height follows around the centre.
    const reachW = Math.max(0, (point.x - ax) * sx);
    // The height is the width over k, and may not pass 1, so the width may not pass k.
    const most = Math.min(k, room(ax, sx));
    const w = clamp(reachW, k * minHeight(k), Math.max(most, k * minHeight(k)));
    const h = w / k;
    return inside({ x: sx > 0 ? ax : ax - w, y: cy - h / 2, w, h });
  }
  const reachH = Math.max(0, (point.y - ay) * sy);
  const most = Math.min(1, room(ay, sy), 1 / k);
  const h = clamp(reachH, minHeight(k), Math.max(most, minHeight(k)));
  const w = k * h;
  return inside({ x: cx - w / 2, y: sy > 0 ? ay : ay - h, w, h });
}

/** One side of the crop set to `to` while keeping ratio `k` (see {@link resizeLocked}). */
export function moveEdgeLocked(crop: CropRect, edge: Edge, to: number, k: number): CropRect {
  const handle: Handle = { left: 'w', right: 'e', top: 'n', bottom: 's' }[edge] as Handle;
  return resizeLocked(crop, handle, { x: to, y: to }, k);
}
