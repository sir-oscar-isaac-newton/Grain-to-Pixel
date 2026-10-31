// The maths of the curve editor. The curve is the one the backend applies (crates/gtp-edit
// curve.rs): a Fritsch-Carlson monotone cubic through the points, held flat beyond the end points.
// Both are checked against the same cases (crates/gtp-edit/tests/fixtures/curve-cases.json).

/** A control point: [input, output], both from 0 to 1. */
export type CurvePoint = [number, number];

/** The most points a curve may have (the backend keeps this many). */
export const CURVE_POINTS_MAX = 16;
/** The least a point may be from its neighbours in input, so that inputs stay strictly increasing. */
export const CURVE_MIN_GAP = 0.001;

/** The curve that changes nothing, as the Master curve starts. */
export const identityCurve = (): CurvePoint[] => [
  [0, 0],
  [1, 1],
];

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/** Tangents of the monotone cubic (the same weighting as the backend). */
function tangents(points: readonly CurvePoint[]): number[] {
  const n = points.length;
  const h: number[] = [];
  const d: number[] = [];
  for (let i = 0; i + 1 < n; i += 1) {
    const a = points[i] as CurvePoint;
    const b = points[i + 1] as CurvePoint;
    h.push(b[0] - a[0]);
    d.push((b[1] - a[1]) / (b[0] - a[0]));
  }
  const m = new Array<number>(n).fill(0);
  m[0] = d[0] as number;
  m[n - 1] = d[n - 2] as number;
  for (let i = 1; i < n - 1; i += 1) {
    const before = d[i - 1] as number;
    const after = d[i] as number;
    if (before * after > 0) {
      const w1 = 2 * (h[i] as number) + (h[i - 1] as number);
      const w2 = (h[i] as number) + 2 * (h[i - 1] as number);
      m[i] = (w1 + w2) / (w1 / before + w2 / after);
    }
  }
  return m;
}

/**
 * The curve's value at `x` (0 to 1) for points that are in the unit square and strictly increasing
 * in input. An empty list is the identity; one point is held flat.
 */
export function curveValue(points: readonly CurvePoint[], x: number): number {
  if (points.length === 0) return x;
  const first = points[0] as CurvePoint;
  const last = points[points.length - 1] as CurvePoint;
  if (points.length === 1 || x <= first[0]) return first[1];
  if (x >= last[0]) return last[1];
  const m = tangents(points);
  let k = 0;
  while (k < points.length - 2 && x >= (points[k + 1] as CurvePoint)[0]) k += 1;
  const a = points[k] as CurvePoint;
  const b = points[k + 1] as CurvePoint;
  const h = b[0] - a[0];
  const t = (x - a[0]) / h;
  const t2 = t * t;
  const t3 = t2 * t;
  const value =
    (2 * t3 - 3 * t2 + 1) * a[1] +
    (t3 - 2 * t2 + t) * h * (m[k] as number) +
    (-2 * t3 + 3 * t2) * b[1] +
    (t3 - t2) * h * (m[k + 1] as number);
  return clamp01(value);
}

/** The curve as an SVG path in a square of `size` units, with y pointing down. */
export function curvePath(points: readonly CurvePoint[], size: number, steps = 64): string {
  const parts: string[] = [];
  for (let i = 0; i <= steps; i += 1) {
    const x = i / steps;
    parts.push(
      `${i === 0 ? 'M' : 'L'}${(x * size).toFixed(2)},${((1 - curveValue(points, x)) * size).toFixed(2)}`,
    );
  }
  return parts.join(' ');
}

/**
 * The points with point `index` moved to (`x`, `y`), kept in the unit square and between its
 * neighbours. The end points keep their place in the order, never crossing a neighbour.
 */
export function movePoint(
  points: readonly CurvePoint[],
  index: number,
  x: number,
  y: number,
): CurvePoint[] {
  if (index < 0 || index >= points.length) return points.map((p) => [...p] as CurvePoint);
  const low = index > 0 ? (points[index - 1] as CurvePoint)[0] + CURVE_MIN_GAP : 0;
  const high = index < points.length - 1 ? (points[index + 1] as CurvePoint)[0] - CURVE_MIN_GAP : 1;
  const next = points.map((p) => [...p] as CurvePoint);
  next[index] = [Math.min(high, Math.max(low, clamp01(x))), clamp01(y)];
  return next;
}

/** The points with a new one at input `x`, on the curve unless `y` is given. Null when it cannot be added. */
export function addPointAt(
  points: readonly CurvePoint[],
  x: number,
  y?: number,
): { points: CurvePoint[]; index: number } | null {
  if (points.length >= CURVE_POINTS_MAX || !Number.isFinite(x)) return null;
  const at = clamp01(x);
  if (points.some((p) => Math.abs(p[0] - at) < CURVE_MIN_GAP)) return null;
  const next = points.map((p) => [...p] as CurvePoint);
  const index = next.findIndex((p) => p[0] > at);
  const spot = index === -1 ? next.length : index;
  next.splice(spot, 0, [at, clamp01(y ?? curveValue(points, at))]);
  return { points: next, index: spot };
}

/** A new point in the middle of the widest gap between points (and the ends 0 and 1). */
export function addPointInGap(
  points: readonly CurvePoint[],
): { points: CurvePoint[]; index: number } | null {
  const inputs = [0, ...points.map((p) => p[0]), 1];
  let best = 0;
  let width = -1;
  for (let i = 0; i + 1 < inputs.length; i += 1) {
    const gap = (inputs[i + 1] as number) - (inputs[i] as number);
    if (gap > width) {
      width = gap;
      best = i;
    }
  }
  return addPointAt(points, (inputs[best] as number) + width / 2);
}

/** The points without point `index`; null when that would leave fewer than two. */
export function removePoint(points: readonly CurvePoint[], index: number): CurvePoint[] | null {
  if (points.length <= 2 || index < 0 || index >= points.length) return null;
  return points.filter((_, i) => i !== index).map((p) => [...p] as CurvePoint);
}
