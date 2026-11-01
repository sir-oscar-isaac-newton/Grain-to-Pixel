// The crop rectangle: fractions of the scan (x, y, width, height), as the recipe stores them. The
// editing here keeps it inside the scan and no smaller than the recipe allows.

export interface CropRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The smallest side of a crop, as a fraction of the scan (the recipe's own limit). */
export const MIN_CROP = 0.01;
/** One arrow key moves the crop this far, as a fraction of the scan. */
export const NUDGE = 0.005;
/** With Shift, this far. */
export const BIG_NUDGE = 0.05;

export const FULL_CROP: CropRect = { x: 0, y: 0, w: 1, h: 1 };

export type Edge = 'left' | 'right' | 'top' | 'bottom';
/** The places a crop can be grabbed: its sides and corners, by compass point. */
export type Handle = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw';

export const clamp = (value: number, low: number, high: number) =>
  Math.min(high, Math.max(low, value));

export interface Edges {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export function edgesOf(crop: CropRect): Edges {
  return { left: crop.x, top: crop.y, right: crop.x + crop.w, bottom: crop.y + crop.h };
}

function fromEdges(edges: Edges): CropRect {
  return {
    x: edges.left,
    y: edges.top,
    w: edges.right - edges.left,
    h: edges.bottom - edges.top,
  };
}

/** The crop kept inside the scan and at least {@link MIN_CROP} on each side. */
export function clampCrop(crop: CropRect): CropRect {
  const w = clamp(Number.isFinite(crop.w) ? crop.w : 1, MIN_CROP, 1);
  const h = clamp(Number.isFinite(crop.h) ? crop.h : 1, MIN_CROP, 1);
  return {
    x: clamp(Number.isFinite(crop.x) ? crop.x : 0, 0, 1 - w),
    y: clamp(Number.isFinite(crop.y) ? crop.y : 0, 0, 1 - h),
    w,
    h,
  };
}

/** The crop moved by (`dx`, `dy`), stopping at the edges of the scan; its size is unchanged. */
export function moveCrop(crop: CropRect, dx: number, dy: number): CropRect {
  return {
    ...crop,
    x: clamp(crop.x + dx, 0, 1 - crop.w),
    y: clamp(crop.y + dy, 0, 1 - crop.h),
  };
}

/** One edge placed at `to`, stopping at the scan's edge and at {@link MIN_CROP} from the opposite one. */
export function moveEdge(crop: CropRect, edge: Edge, to: number): CropRect {
  const edges = edgesOf(crop);
  const at = Number.isFinite(to) ? to : edges[edge];
  switch (edge) {
    case 'left':
      edges.left = clamp(at, 0, edges.right - MIN_CROP);
      break;
    case 'right':
      edges.right = clamp(at, edges.left + MIN_CROP, 1);
      break;
    case 'top':
      edges.top = clamp(at, 0, edges.bottom - MIN_CROP);
      break;
    case 'bottom':
      edges.bottom = clamp(at, edges.top + MIN_CROP, 1);
      break;
  }
  return fromEdges(edges);
}

/** The crop with the edges of `handle` pulled to `point` (fractions of the scan). */
export function resizeByHandle(crop: CropRect, handle: Handle, point: { x: number; y: number }) {
  let next = crop;
  if (handle.includes('n')) next = moveEdge(next, 'top', point.y);
  if (handle.includes('s')) next = moveEdge(next, 'bottom', point.y);
  if (handle.includes('w')) next = moveEdge(next, 'left', point.x);
  if (handle.includes('e')) next = moveEdge(next, 'right', point.x);
  return next;
}

/** The crop moved one step by an arrow key (`null` for any other key). */
export function nudgeCrop(crop: CropRect, key: string, big: boolean): CropRect | null {
  const step = big ? BIG_NUDGE : NUDGE;
  switch (key) {
    case 'ArrowLeft':
      return moveCrop(crop, -step, 0);
    case 'ArrowRight':
      return moveCrop(crop, step, 0);
    case 'ArrowUp':
      return moveCrop(crop, 0, -step);
    case 'ArrowDown':
      return moveCrop(crop, 0, step);
    default:
      return null;
  }
}

/** Whether two crops are the same to within what the recipe can tell apart. */
export function sameCrop(a: CropRect, b: CropRect): boolean {
  const close = (p: number, q: number) => Math.abs(p - q) < 1e-6;
  return close(a.x, b.x) && close(a.y, b.y) && close(a.w, b.w) && close(a.h, b.h);
}
