import { describe, expect, it } from 'vitest';
import {
  CLICK_FRACTION,
  MIN_BOX_PX,
  boxRegion,
  clickRegion,
  distance,
  pointOnPicture,
} from './picker';

/** A picture of 400 x 300 pixels with its corner at (100, 50) on the screen. */
const picture = { left: 100, top: 50, width: 400, height: 300 };

describe('picker', () => {
  it('finds a screen point on the picture as fractions of it, or nothing outside', () => {
    expect(pointOnPicture({ x: 300, y: 200 }, picture)).toEqual({ x: 0.5, y: 0.5 });
    expect(pointOnPicture({ x: 100, y: 50 }, picture)).toEqual({ x: 0, y: 0 });
    expect(pointOnPicture({ x: 500, y: 350 }, picture)).toEqual({ x: 1, y: 1 });
    for (const outside of [
      { x: 99, y: 200 },
      { x: 501, y: 200 },
      { x: 300, y: 49 },
      { x: 300, y: 351 },
    ]) {
      expect(pointOnPicture(outside, picture)).toBeNull();
    }
    expect(pointOnPicture({ x: 1, y: 1 }, { ...picture, width: 0 })).toBeNull();
  });

  it('turns a click into a small square around it, square on the screen', () => {
    const region = clickRegion({ x: 300, y: 200 }, picture);
    expect(region).not.toBeNull();
    expect(region?.w).toBe(CLICK_FRACTION);
    // 2% of 400 px is 8 px; as a fraction of the 300 px height that is a bit more than 2%.
    expect(region?.h).toBeCloseTo((CLICK_FRACTION * 400) / 300, 6);
    expect((region?.w ?? 0) * 400).toBeCloseTo((region?.h ?? 0) * 300, 6);
    // Centred on the click.
    expect((region?.x ?? 0) + (region?.w ?? 0) / 2).toBeCloseTo(0.5, 6);
    expect((region?.y ?? 0) + (region?.h ?? 0) / 2).toBeCloseTo(0.5, 6);
  });

  it('keeps the square inside the picture when the click is at an edge or a corner', () => {
    for (const corner of [
      { x: 100, y: 50 },
      { x: 500, y: 50 },
      { x: 100, y: 350 },
      { x: 500, y: 350 },
      { x: 300, y: 50 },
    ]) {
      const region = clickRegion(corner, picture);
      expect(region).not.toBeNull();
      const r = region ?? { x: 0, y: 0, w: 0, h: 0 };
      expect(r.x).toBeGreaterThanOrEqual(0);
      expect(r.y).toBeGreaterThanOrEqual(0);
      expect(r.x + r.w).toBeLessThanOrEqual(1 + 1e-9);
      expect(r.y + r.h).toBeLessThanOrEqual(1 + 1e-9);
    }
    expect(clickRegion({ x: 0, y: 0 }, picture)).toBeNull();
  });

  it('covers a dragged box, whichever way it was dragged', () => {
    const forward = boxRegion({ x: 200, y: 100 }, { x: 300, y: 200 }, picture);
    expect(forward).toEqual({ x: 0.25, y: 50 / 300, w: 0.25, h: 100 / 300 });
    const backward = boxRegion({ x: 300, y: 200 }, { x: 200, y: 100 }, picture);
    expect(backward).toEqual(forward);
  });

  it('cuts a box off at the edges of the picture', () => {
    const region = boxRegion({ x: 0, y: 0 }, { x: 300, y: 200 }, picture);
    expect(region).toEqual({ x: 0, y: 0, w: 0.5, h: 0.5 });
    const far = boxRegion({ x: 400, y: 300 }, { x: 900, y: 900 }, picture);
    expect(far).toEqual({ x: 0.75, y: 250 / 300, w: 0.25, h: 50 / 300 });
  });

  it('gives nothing for a box that is outside the picture or thinner than a couple of pixels', () => {
    expect(boxRegion({ x: 0, y: 0 }, { x: 90, y: 40 }, picture)).toBeNull();
    expect(boxRegion({ x: 520, y: 100 }, { x: 600, y: 200 }, picture)).toBeNull();
    expect(boxRegion({ x: 200, y: 100 }, { x: 200 + MIN_BOX_PX - 1, y: 200 }, picture)).toBeNull();
    expect(boxRegion({ x: 200, y: 100 }, { x: 300, y: 100 + MIN_BOX_PX - 1 }, picture)).toBeNull();
    expect(boxRegion({ x: 200, y: 100 }, { x: 300, y: 200 }, { ...picture, height: 0 })).toBeNull();
  });

  it('measures how far a press moved', () => {
    expect(distance({ x: 0, y: 0 }, { x: 3, y: 4 })).toBe(5);
    expect(distance({ x: 5, y: 5 }, { x: 5, y: 5 })).toBe(0);
  });
});
