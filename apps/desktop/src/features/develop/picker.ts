// Picking on the picture: a click or a dragged box becomes a region of the whole scan, given as
// fractions of it (x, y, width, height, each from 0 to 1), which is what the backend samples.
import type { Point } from './viewport';

/** A rectangle on the screen, in pixels. */
export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** A region of the picture in fractions of it: x, y, width and height, each from 0 to 1. */
export interface Fractions {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A click covers a square this wide, as a fraction of the picture's width. */
export const CLICK_FRACTION = 0.02;
/** A press that moves less than this many pixels is a click, not the start of a box. */
export const DRAG_THRESHOLD_PX = 4;
/** The smallest side of a dragged box, in pixels: a thinner one is taken as a click. */
export const MIN_BOX_PX = 2;

const clamp01 = (value: number) => Math.min(1, Math.max(0, value));

/** Where a screen point is on the picture, as fractions of it; null when it is outside it. */
export function pointOnPicture(point: Point, picture: Box): Point | null {
  if (picture.width <= 0 || picture.height <= 0) return null;
  const x = (point.x - picture.left) / picture.width;
  const y = (point.y - picture.top) / picture.height;
  if (x < 0 || x > 1 || y < 0 || y > 1) return null;
  return { x, y };
}

/**
 * The square a click covers, centred on it and kept inside the picture. It is square on screen,
 * so its height as a fraction depends on the picture's shape.
 */
export function clickRegion(point: Point, picture: Box): Fractions | null {
  const at = pointOnPicture(point, picture);
  if (!at) return null;
  const w = CLICK_FRACTION;
  const h = Math.min(1, (CLICK_FRACTION * picture.width) / picture.height);
  return {
    x: Math.min(1 - w, Math.max(0, at.x - w / 2)),
    y: Math.min(1 - h, Math.max(0, at.y - h / 2)),
    w,
    h,
  };
}

/**
 * The region a box dragged from `from` to `to` covers, cut off at the edges of the picture. Null
 * when it is not on the picture at all or is thinner than {@link MIN_BOX_PX}.
 */
export function boxRegion(from: Point, to: Point, picture: Box): Fractions | null {
  if (picture.width <= 0 || picture.height <= 0) return null;
  const left = Math.max(picture.left, Math.min(from.x, to.x));
  const right = Math.min(picture.left + picture.width, Math.max(from.x, to.x));
  const top = Math.max(picture.top, Math.min(from.y, to.y));
  const bottom = Math.min(picture.top + picture.height, Math.max(from.y, to.y));
  if (right - left < MIN_BOX_PX || bottom - top < MIN_BOX_PX) return null;
  return {
    x: clamp01((left - picture.left) / picture.width),
    y: clamp01((top - picture.top) / picture.height),
    w: (right - left) / picture.width,
    h: (bottom - top) / picture.height,
  };
}

/** How far a press has moved, in pixels. */
export function distance(a: Point, b: Point): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}
