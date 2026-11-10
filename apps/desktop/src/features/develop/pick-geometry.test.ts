import { describe, expect, it } from 'vitest';
import {
  pointToFrame,
  regionToFrame,
  regionToScan,
  straightSize,
  type PickGeometry,
} from './pick-geometry';

const SCAN = { width: 6000, height: 4000 };
const WHOLE = { x: 0, y: 0, w: 1, h: 1 };
const plain: PickGeometry = { crop: WHOLE, angle_deg: 0, rot90: 0, flip_h: false, flip_v: false };

/** The forward transform of crates/gtp-edit/src/geometry.rs, on a point in fractions of the crop. */
function forward(point: { x: number; y: number }, g: PickGeometry) {
  const cw = Math.round((g.crop.x + g.crop.w) * SCAN.width) - Math.round(g.crop.x * SCAN.width);
  const ch = Math.round((g.crop.y + g.crop.h) * SCAN.height) - Math.round(g.crop.y * SCAN.height);
  const s = straightSize(cw, ch, g.angle_deg);
  const theta = (g.angle_deg * Math.PI) / 180;
  const dx = (point.x - 0.5) * cw;
  const dy = (point.y - 0.5) * ch;
  // Straighten: the source position is centre + R(u, v), so (u, v) is R transposed.
  let x = (dx * Math.cos(theta) - dy * Math.sin(theta)) / s.w + 0.5;
  let y = (dx * Math.sin(theta) + dy * Math.cos(theta)) / s.h + 0.5;
  // Quarter turns clockwise.
  const turns = ((g.rot90 % 4) + 4) % 4;
  if (turns === 1) [x, y] = [1 - y, x];
  else if (turns === 2) [x, y] = [1 - x, 1 - y];
  else if (turns === 3) [x, y] = [y, 1 - x];
  if (g.flip_h) x = 1 - x;
  if (g.flip_v) y = 1 - y;
  return { x, y };
}

describe('pointToFrame', () => {
  it('is the identity without any geometry', () => {
    expect(pointToFrame({ x: 0.3, y: 0.7 }, plain, SCAN)).toEqual({ x: 0.3, y: 0.7 });
  });

  it('undoes each quarter turn exactly', () => {
    // The top-left corner of the frame is at top-right after one turn, bottom-right after two,
    // bottom-left after three.
    const shown = [
      { x: 0, y: 0 },
      { x: 1, y: 0 },
      { x: 1, y: 1 },
      { x: 0, y: 1 },
    ];
    shown.forEach((at, turns) => {
      expect(pointToFrame(at, { ...plain, rot90: turns }, SCAN)).toEqual({ x: 0, y: 0 });
    });
    // One turn clockwise sends the frame point (0.2, 0.9) to (0.1, 0.2).
    const back = pointToFrame({ x: 0.1, y: 0.2 }, { ...plain, rot90: 1 }, SCAN);
    expect(back.x).toBeCloseTo(0.2, 12);
    expect(back.y).toBeCloseTo(0.9, 12);
  });

  it('wraps turns, so -1 is 3 and 5 is 1', () => {
    const p = { x: 0.2, y: 0.6 };
    expect(pointToFrame(p, { ...plain, rot90: -1 }, SCAN)).toEqual(
      pointToFrame(p, { ...plain, rot90: 3 }, SCAN),
    );
    expect(pointToFrame(p, { ...plain, rot90: 5 }, SCAN)).toEqual(
      pointToFrame(p, { ...plain, rot90: 1 }, SCAN),
    );
  });

  it('undoes the flips, one and both', () => {
    const h = pointToFrame({ x: 0.2, y: 0.3 }, { ...plain, flip_h: true }, SCAN);
    expect(h.x).toBeCloseTo(0.8, 12);
    expect(h.y).toBeCloseTo(0.3, 12);
    const v = pointToFrame({ x: 0.2, y: 0.3 }, { ...plain, flip_v: true }, SCAN);
    expect(v.x).toBeCloseTo(0.2, 12);
    expect(v.y).toBeCloseTo(0.7, 12);
    const both = pointToFrame({ x: 0.2, y: 0.3 }, { ...plain, flip_h: true, flip_v: true }, SCAN);
    expect(both.x).toBeCloseTo(0.8, 12);
    expect(both.y).toBeCloseTo(0.7, 12);
  });

  it('flips act in the final orientation, after the turn', () => {
    // Turn once, then flip left to right: the frame's top-left corner ends at top-left.
    const g = { ...plain, rot90: 1, flip_h: true };
    expect(pointToFrame({ x: 0, y: 0 }, g, SCAN)).toEqual({ x: 0, y: 0 });
    // Both flips with a turn: the frame's top-left ends at bottom-left.
    const both = { ...plain, rot90: 1, flip_h: true, flip_v: true };
    expect(pointToFrame({ x: 0, y: 1 }, both, SCAN)).toEqual({ x: 0, y: 0 });
  });

  it('round-trips through the forward transform for every turn and flip, with an angle and a crop', () => {
    const points = [
      { x: 0.5, y: 0.5 },
      { x: 0.3, y: 0.4 },
      { x: 0.55, y: 0.45 },
      { x: 0.45, y: 0.6 },
    ];
    for (const rot90 of [0, 1, 2, 3]) {
      for (const flip_h of [false, true]) {
        for (const flip_v of [false, true]) {
          for (const angle_deg of [0, 7, -12.5]) {
            const g: PickGeometry = {
              crop: { x: 0.1, y: 0.2, w: 0.7, h: 0.6 },
              angle_deg,
              rot90,
              flip_h,
              flip_v,
            };
            for (const source of points) {
              const back = pointToFrame(forward(source, g), g, SCAN);
              expect(back.x).toBeCloseTo(source.x, 9);
              expect(back.y).toBeCloseTo(source.y, 9);
            }
          }
        }
      }
    }
  });

  it('works in pixels, so a rotation of a wide frame is not skewed by its aspect ratio', () => {
    // 6000 x 4000 scan, angle 10: a point 0.1 of the straightened width right of the centre is
    // 0.1 * straight width pixels right, rotated; in fractions the two axes scale differently.
    const g = { ...plain, angle_deg: 10 };
    const s = straightSize(6000, 4000, 10);
    const at = pointToFrame({ x: 0.6, y: 0.5 }, g, SCAN);
    const theta = (10 * Math.PI) / 180;
    const u = 0.1 * s.w;
    expect(at.x).toBeCloseTo(0.5 + (u * Math.cos(theta)) / 6000, 12);
    expect(at.y).toBeCloseTo(0.5 - (u * Math.sin(theta)) / 4000, 12);
  });
});

describe('straightSize', () => {
  it('is the crop at angle 0 and the inscribed rectangle of the same aspect otherwise', () => {
    expect(straightSize(6000, 4000, 0)).toEqual({ w: 6000, h: 4000 });
    const s = straightSize(6000, 4000, 10);
    expect(s.w).toBeLessThan(6000);
    expect(s.w / s.h).toBeCloseTo(1.5, 2);
  });
});

describe('regionToFrame', () => {
  it('maps a box exactly under turns and flips', () => {
    const region = { x: 0.1, y: 0.2, w: 0.3, h: 0.4 };
    // Flipped left to right: the box moves to the other side.
    const flipped = regionToFrame(region, { ...plain, flip_h: true }, SCAN);
    expect(flipped.x).toBeCloseTo(0.6, 12);
    expect(flipped.y).toBeCloseTo(0.2, 12);
    expect(flipped.w).toBeCloseTo(0.3, 12);
    expect(flipped.h).toBeCloseTo(0.4, 12);
    // One turn clockwise: the box has its width and height swapped.
    const turned = regionToFrame(region, { ...plain, rot90: 1 }, SCAN);
    expect(turned.x).toBeCloseTo(0.2, 12);
    expect(turned.y).toBeCloseTo(0.6, 12);
    expect(turned.w).toBeCloseTo(0.4, 12);
    expect(turned.h).toBeCloseTo(0.3, 12);
  });

  it('gives the smallest upright box around the tilted one when there is an angle', () => {
    const g = { ...plain, angle_deg: 20 };
    const box = regionToFrame({ x: 0.4, y: 0.4, w: 0.2, h: 0.2 }, g, SCAN);
    const corners = [
      { x: 0.4, y: 0.4 },
      { x: 0.6, y: 0.4 },
      { x: 0.4, y: 0.6 },
      { x: 0.6, y: 0.6 },
    ].map((c) => pointToFrame(c, g, SCAN));
    expect(box.x).toBeCloseTo(Math.min(...corners.map((c) => c.x)), 12);
    expect(box.y).toBeCloseTo(Math.min(...corners.map((c) => c.y)), 12);
    expect(box.x + box.w).toBeCloseTo(Math.max(...corners.map((c) => c.x)), 12);
    expect(box.y + box.h).toBeCloseTo(Math.max(...corners.map((c) => c.y)), 12);
    // It contains the mapped centre.
    const centre = pointToFrame({ x: 0.5, y: 0.5 }, g, SCAN);
    expect(centre.x).toBeGreaterThan(box.x);
    expect(centre.x).toBeLessThan(box.x + box.w);
  });

  it('keeps the box inside the frame', () => {
    const g = { ...plain, angle_deg: 30, rot90: 2 };
    const box = regionToFrame({ x: 0, y: 0, w: 1, h: 1 }, g, SCAN);
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.x + box.w).toBeLessThanOrEqual(1 + 1e-12);
    expect(box.y + box.h).toBeLessThanOrEqual(1 + 1e-12);
    // A box that reaches past the picture maps past the frame, and is cut off at its edge.
    const over = regionToFrame({ x: 0.9, y: 0.9, w: 0.5, h: 0.5 }, plain, SCAN);
    expect(over.x).toBeCloseTo(0.9, 12);
    expect(over.x + over.w).toBe(1);
    expect(over.y + over.h).toBe(1);
    const before = regionToFrame({ x: -0.2, y: -0.2, w: 0.3, h: 0.3 }, plain, SCAN);
    expect([before.x, before.y]).toEqual([0, 0]);
  });
});

describe('regionToScan', () => {
  it('undoes the turn and flips of the whole scan, and ignores any crop or angle', () => {
    const region = { x: 0.1, y: 0.2, w: 0.3, h: 0.4 };
    const box = regionToScan(region, { rot90: 1, flip_h: true, flip_v: false }, SCAN);
    expect(box).toEqual(regionToFrame(region, { ...plain, rot90: 1, flip_h: true }, SCAN));
  });
});
