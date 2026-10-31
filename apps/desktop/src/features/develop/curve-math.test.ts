import { describe, expect, it } from 'vitest';
import fixture from '../../../../../crates/gtp-edit/tests/fixtures/curve-cases.json';
import {
  CURVE_MIN_GAP,
  CURVE_POINTS_MAX,
  addPointAt,
  addPointInGap,
  curvePath,
  curveValue,
  identityCurve,
  movePoint,
  removePoint,
  type CurvePoint,
} from '@gtp/ui';

interface Case {
  points: CurvePoint[];
  samples: Array<[number, number]>;
}

// The same cases the backend's curve is checked against (crates/gtp-edit/src/curve.rs).
const cases = fixture as Case[];

describe('curveValue', () => {
  it.each(cases.map((c, i) => [i, c] as const))(
    'matches the backend’s curve for case %i',
    (_i, { points, samples }) => {
      expect(samples.length).toBeGreaterThan(10);
      for (const [x, want] of samples) {
        expect(curveValue(points, x), `${JSON.stringify(points)} at ${x}`).toBeCloseTo(want, 3);
      }
    },
  );

  it('is the identity for no points, and the default line is the identity too', () => {
    for (const x of [0, 0.3, 1]) {
      expect(curveValue([], x)).toBe(x);
      expect(curveValue(identityCurve(), x)).toBeCloseTo(x, 9);
    }
  });

  it('passes through its points and is held flat beyond the end points', () => {
    const points: CurvePoint[] = [
      [0.2, 0.1],
      [0.5, 0.6],
      [0.8, 0.9],
    ];
    for (const [x, y] of points) expect(curveValue(points, x)).toBeCloseTo(y, 9);
    expect(curveValue(points, 0)).toBe(0.1);
    expect(curveValue(points, 1)).toBe(0.9);
  });

  it('does not overshoot: monotone points give a monotone curve inside 0 to 1', () => {
    const points: CurvePoint[] = [
      [0, 0],
      [0.1, 0.5],
      [0.2, 0.52],
      [0.9, 0.55],
      [1, 1],
    ];
    let last = -1;
    for (let i = 0; i <= 200; i += 1) {
      const v = curveValue(points, i / 200);
      expect(v).toBeGreaterThanOrEqual(last);
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThanOrEqual(1);
      last = v;
    }
  });
});

describe('curvePath', () => {
  it('draws from the left edge to the right edge with y pointing down', () => {
    const path = curvePath(identityCurve(), 100, 4);
    expect(path).toBe('M0.00,100.00 L25.00,75.00 L50.00,50.00 L75.00,25.00 L100.00,0.00');
  });
});

describe('editing points', () => {
  const three: CurvePoint[] = [
    [0, 0],
    [0.5, 0.5],
    [1, 1],
  ];

  it('moves a point, keeping it in the square and between its neighbours', () => {
    expect(movePoint(three, 1, 0.6, 0.7)[1]).toEqual([0.6, 0.7]);
    expect(movePoint(three, 1, 5, -3)[1]).toEqual([1 - CURVE_MIN_GAP, 0]);
    expect(movePoint(three, 1, -5, 3)[1]).toEqual([CURVE_MIN_GAP, 1]);
    expect(movePoint(three, 0, -1, 0.2)[0]).toEqual([0, 0.2]);
    expect(movePoint(three, 2, 2, 2)[2]).toEqual([1, 1]);
    // Nothing is changed in place, and a bad index changes nothing.
    expect(three[1]).toEqual([0.5, 0.5]);
    expect(movePoint(three, 9, 0.1, 0.1)).toEqual(three);
  });

  it('adds a point on the curve, in order, and never on top of another', () => {
    const added = addPointAt(three, 0.25);
    expect(added?.index).toBe(1);
    expect(added?.points).toHaveLength(4);
    expect(added?.points[1]?.[0]).toBe(0.25);
    expect(added?.points[1]?.[1]).toBeCloseTo(curveValue(three, 0.25), 9);
    expect(addPointAt(three, 0.7, 0.2)?.points[2]).toEqual([0.7, 0.2]);
    expect(addPointAt(three, 0.5)).toBeNull();
    expect(addPointAt(three, 0.5 + CURVE_MIN_GAP / 2)).toBeNull();
    expect(addPointAt(three, Number.NaN)).toBeNull();
  });

  it('adds a point in the middle of the widest gap', () => {
    const wide: CurvePoint[] = [
      [0.1, 0.1],
      [0.2, 0.2],
      [0.9, 0.9],
    ];
    expect(addPointInGap(wide)?.points[2]?.[0]).toBeCloseTo(0.55, 9);
    expect(addPointInGap(identityCurve())?.points.map((p) => p[0])).toEqual([0, 0.5, 1]);
    // The gap before the first point counts too.
    const late: CurvePoint[] = [
      [0.8, 0.8],
      [0.9, 0.9],
    ];
    expect(addPointInGap(late)?.index).toBe(0);
  });

  it('stops at the most points a curve may have', () => {
    let points: CurvePoint[] = identityCurve();
    for (let i = points.length; i < CURVE_POINTS_MAX; i += 1) {
      points = addPointInGap(points)?.points ?? points;
    }
    expect(points).toHaveLength(CURVE_POINTS_MAX);
    expect(addPointInGap(points)).toBeNull();
    for (let i = 1; i < points.length; i += 1) {
      expect((points[i] as CurvePoint)[0]).toBeGreaterThan((points[i - 1] as CurvePoint)[0]);
    }
  });

  it('removes a point, but never leaves fewer than two', () => {
    expect(removePoint(three, 1)).toEqual([
      [0, 0],
      [1, 1],
    ]);
    expect(removePoint(identityCurve(), 0)).toBeNull();
    expect(removePoint(three, 7)).toBeNull();
    expect(three).toHaveLength(3);
  });
});
