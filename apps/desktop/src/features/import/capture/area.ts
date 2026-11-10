/** The calibration area: a rectangle in fractions (0 to 1) of the whole frame. */
export interface Area {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A pointer moves less than this many pixels between press and release: it is a click. */
export const DRAG_THRESHOLD_PX = 4;

const clamp = (v: number) => Math.min(1, Math.max(0, v));

/** The rectangle between two points (fractions of the frame), clamped to the frame. */
export function areaBetween(a: { x: number; y: number }, b: { x: number; y: number }): Area {
  const x0 = clamp(Math.min(a.x, b.x));
  const y0 = clamp(Math.min(a.y, b.y));
  const x1 = clamp(Math.max(a.x, b.x));
  const y1 = clamp(Math.max(a.y, b.y));
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** The edges of an area as whole percentages, for words. */
export function areaPercents({ x, y, w, h }: Area) {
  return {
    x0: Math.round(x * 100),
    x1: Math.round((x + w) * 100),
    y0: Math.round(y * 100),
    y1: Math.round((y + h) * 100),
  };
}
