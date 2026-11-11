import { describe, expect, it } from 'vitest';
import type { Detail } from '../../shared/ipc';
import {
  DETAIL_MAX_EDGE,
  contains,
  coveredPart,
  requestRegion,
  visiblePart,
} from './detail-region';
import { MAX_ZOOM, ceilingFor, isOneToOne, oneToOneZoom, zoomLimits } from './viewport';

const stage = { width: 900, height: 600 };
// The preview is 2048 wide at the zoom unit, so a 3:2 picture is 2048 x 1365.33 at zoom 1.
const picture = { width: 1024, height: 683 };

describe('the zoom at 1:1', () => {
  it('is the full picture over the zoom unit, and lifts the largest zoom to 200% of the scan', () => {
    expect(oneToOneZoom(6000)).toBeCloseTo(6000 / 2048, 10);
    expect(oneToOneZoom(0)).toBe(0);
    expect(ceilingFor(oneToOneZoom(6000))).toBeCloseTo(2 * (6000 / 2048), 10);
    // A small scan keeps the usual 400%.
    expect(ceilingFor(oneToOneZoom(2048))).toBe(MAX_ZOOM);
    expect(zoomLimits(stage, picture, 6).max).toBe(6);
    expect(zoomLimits(stage, picture).max).toBe(MAX_ZOOM);
  });

  it('is recognised to what a readout can tell apart', () => {
    expect(isOneToOne(2.93, 2.93)).toBe(true);
    expect(isOneToOne(2.9301, 2.93)).toBe(true);
    expect(isOneToOne(2.95, 2.93)).toBe(false);
    expect(isOneToOne(1, 0)).toBe(false);
  });
});

describe('visiblePart', () => {
  it('is the whole picture when it fits, and the middle of it when zoomed in', () => {
    const fit = { fit: false, zoom: 0.4, offset: { x: 0, y: 0 } };
    expect(visiblePart(fit, stage, picture)).toEqual({ x: 0, y: 0, w: 1, h: 1 });
    const zoomed = visiblePart({ fit: false, zoom: 2, offset: { x: 0, y: 0 } }, stage, picture);
    // 4096 x 2731 on screen, 900 x 600 of it visible.
    expect(zoomed?.w).toBeCloseTo(900 / 4096, 6);
    expect(zoomed?.h).toBeCloseTo(600 / 2732, 6);
    expect((zoomed?.x ?? 0) + (zoomed?.w ?? 0) / 2).toBeCloseTo(0.5, 6);
  });

  it('follows the pan: moving the picture left shows what is to the right', () => {
    const centred = visiblePart({ fit: false, zoom: 2, offset: { x: 0, y: 0 } }, stage, picture);
    const moved = visiblePart({ fit: false, zoom: 2, offset: { x: -400, y: 0 } }, stage, picture);
    expect((moved?.x ?? 0) - (centred?.x ?? 0)).toBeCloseTo(400 / 4096, 6);
  });

  it('is null without a stage or a picture', () => {
    const view = { fit: false, zoom: 2, offset: { x: 0, y: 0 } };
    expect(visiblePart(view, { width: 0, height: 0 }, picture)).toBeNull();
    expect(visiblePart(view, stage, { width: 0, height: 0 })).toBeNull();
  });
});

describe('requestRegion', () => {
  const full = { width: 6000, height: 4000 };

  it('grows the visible part by half of it on each side, around its centre', () => {
    const region = requestRegion({ x: 0.4, y: 0.4, w: 0.1, h: 0.1 }, full);
    expect(region.w).toBeCloseTo(0.2, 6);
    expect(region.h).toBeCloseTo(0.2, 6);
    expect(region.x).toBeCloseTo(0.35, 3);
    expect(region.y).toBeCloseTo(0.35, 3);
  });

  it('stays inside the picture at the borders', () => {
    const corner = requestRegion({ x: 0, y: 0, w: 0.1, h: 0.1 }, full);
    expect(corner.x).toBe(0);
    expect(corner.y).toBe(0);
    expect(corner.w).toBeCloseTo(0.2, 6);
    const far = requestRegion({ x: 0.9, y: 0.9, w: 0.1, h: 0.1 }, full);
    expect(far.x + far.w).toBeCloseTo(1, 9);
    expect(far.y + far.h).toBeCloseTo(1, 9);
  });

  it('never spans more than the cap, and is whole pixels', () => {
    const region = requestRegion({ x: 0, y: 0, w: 1, h: 1 }, full);
    expect(region.w * full.width).toBeLessThanOrEqual(DETAIL_MAX_EDGE);
    expect(region.h * full.height).toBeLessThanOrEqual(DETAIL_MAX_EDGE);
    expect(DETAIL_MAX_EDGE).toBeLessThan(4096);
    const odd = requestRegion({ x: 0.123, y: 0.321, w: 0.0777, h: 0.0555 }, full);
    expect(Number.isInteger(Math.round(odd.x * full.width * 1e6) / 1e6)).toBe(true);
    expect(Number.isInteger(Math.round(odd.w * full.width * 1e6) / 1e6)).toBe(true);
  });
});

describe('what a detail covers', () => {
  const detail: Detail = {
    seq: 1,
    width: 1500,
    height: 1000,
    x: 1500,
    y: 1000,
    fullWidth: 6000,
    fullHeight: 4000,
    rgba: new Uint8ClampedArray(0),
  };

  it('is its rectangle in fractions, and holds what lies inside it', () => {
    const covered = coveredPart(detail);
    expect(covered).toEqual({ x: 0.25, y: 0.25, w: 0.25, h: 0.25 });
    expect(contains(covered, { x: 0.3, y: 0.3, w: 0.1, h: 0.1 })).toBe(true);
    expect(contains(covered, covered)).toBe(true);
    expect(contains(covered, { x: 0.3, y: 0.3, w: 0.21, h: 0.1 })).toBe(false);
    expect(contains(covered, { x: 0.2, y: 0.3, w: 0.1, h: 0.1 })).toBe(false);
  });
});
