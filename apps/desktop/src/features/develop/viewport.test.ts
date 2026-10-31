import { describe, expect, it } from 'vitest';
import {
  FIT_VIEW,
  MAX_ZOOM,
  PROXY_EDGE,
  ZOOM_STEP,
  actualSize,
  clampOffset,
  fitZoom,
  panBy,
  referenceSize,
  resolveView,
  zoomLimits,
  zoomTo,
} from './viewport';

const stage = { width: 1000, height: 600 };
/** A 3:2 preview, at either size. */
const big = { width: 2048, height: 1365 };
const small = { width: 1600, height: 1067 };

describe('viewport', () => {
  it('measures the zoom against the size at the proxy edge, whatever the preview size', () => {
    const a = referenceSize(big);
    const b = referenceSize(small);
    expect(Math.max(a.width, a.height)).toBe(PROXY_EDGE);
    expect(Math.max(b.width, b.height)).toBe(PROXY_EDGE);
    expect(b.width / b.height).toBeCloseTo(a.width / a.height, 2);
    expect(referenceSize({ width: 0, height: 0 })).toEqual({ width: 0, height: 0 });
  });

  it('fits the whole image in the stage, and the same for a 1600 px preview as a 2048 px one', () => {
    // 2048 x 1365 in 1000 x 600: limited by the height.
    expect(fitZoom(stage, big)).toBeCloseTo(600 / 1365, 4);
    expect(fitZoom(stage, small)).toBeCloseTo(fitZoom(stage, big), 3);
    // A tall image is limited by the other side.
    expect(fitZoom(stage, { width: 1000, height: 1500 })).toBeCloseTo(600 / 2048, 4);
    // Nothing to fit yet.
    expect(fitZoom({ width: 0, height: 0 }, big)).toBe(1);
    expect(fitZoom(stage, { width: 0, height: 0 })).toBe(1);
  });

  it('limits the zoom to the fit zoom and 400%', () => {
    const fit = fitZoom(stage, big);
    expect(zoomLimits(stage, big)).toEqual({ min: fit, max: MAX_ZOOM });
    // A stage larger than the image fits above 100%; the limits never cross.
    const huge = { width: 12000, height: 9000 };
    const { min, max } = zoomLimits(huge, big);
    expect(min).toBeGreaterThan(MAX_ZOOM);
    expect(max).toBe(min);
  });

  it('follows the stage while it fits, and clamps a zoomed view into it', () => {
    expect(resolveView(FIT_VIEW, stage, big)).toEqual({
      fit: true,
      zoom: fitZoom(stage, big),
      offset: { x: 0, y: 0 },
    });
    const wild = resolveView({ fit: false, zoom: 99, offset: { x: 1e6, y: -1e6 } }, stage, big);
    expect(wild.zoom).toBe(MAX_ZOOM);
    expect(wild.offset.x).toBeCloseTo((2048 * MAX_ZOOM - 1000) / 2, 3);
    expect(wild.offset.y).toBeCloseTo(-(1365 * MAX_ZOOM - 600) / 2, 3);
    expect(resolveView({ fit: false, zoom: 0.0001, offset: { x: 5, y: 5 } }, stage, big).zoom).toBe(
      fitZoom(stage, big),
    );
  });

  it('keeps an image centred on an axis it does not fill, and inside the stage on one it does', () => {
    // At 50% the image is 1024 x 683: wider than the stage's 1000, shorter than its 600? no: 683 > 600.
    const half = clampOffset({ x: 500, y: 500 }, 0.5, stage, big);
    expect(half.x).toBeCloseTo((1024 - 1000) / 2, 3);
    expect(half.y).toBeCloseTo((682.5 - 600) / 2, 3);
    // At 25% it is smaller than the stage on both axes: no room to move.
    expect(clampOffset({ x: 300, y: -300 }, 0.25, stage, big)).toEqual({ x: 0, y: 0 });
  });

  it('zooms around the point under the cursor, which stays where it is', () => {
    const cursor = { x: 200, y: -100 };
    const before = resolveView(FIT_VIEW, stage, big);
    const zoomed = zoomTo(FIT_VIEW, before.zoom * 2, cursor, stage, big);
    expect(zoomed.fit).toBe(false);
    expect(zoomed.zoom).toBeCloseTo(before.zoom * 2, 6);
    // The image point under the cursor before: (cursor - offset) / zoom. It must not move.
    const pointBefore = {
      x: (cursor.x - before.offset.x) / before.zoom,
      y: (cursor.y - before.offset.y) / before.zoom,
    };
    const pointAfter = {
      x: (cursor.x - zoomed.offset.x) / zoomed.zoom,
      y: (cursor.y - zoomed.offset.y) / zoomed.zoom,
    };
    // Unless the clamp moved it (the image must stay on the stage): here the zoom is large enough.
    expect(pointAfter.x).toBeCloseTo(pointBefore.x, 3);
    expect(pointAfter.y).toBeCloseTo(pointBefore.y, 3);
  });

  it('goes back to a fitting view when zoomed out to the fit zoom, and cannot zoom past 400%', () => {
    const fit = fitZoom(stage, big);
    const zoomed = zoomTo(FIT_VIEW, fit * ZOOM_STEP, { x: 0, y: 0 }, stage, big);
    expect(zoomTo(zoomed, fit, { x: 0, y: 0 }, stage, big)).toEqual(FIT_VIEW);
    expect(zoomTo(zoomed, fit / 3, { x: 0, y: 0 }, stage, big)).toEqual(FIT_VIEW);
    expect(zoomTo(zoomed, 100, { x: 0, y: 0 }, stage, big).zoom).toBe(MAX_ZOOM);
  });

  it('pans a zoomed view within the image, and does not move a fitting one', () => {
    expect(panBy(FIT_VIEW, { x: 50, y: 50 }, stage, big)).toBe(FIT_VIEW);
    const zoomed = zoomTo(FIT_VIEW, 1, { x: 0, y: 0 }, stage, big);
    const moved = panBy(zoomed, { x: -30, y: 20 }, stage, big);
    expect(moved.offset).toEqual({ x: -30, y: 20 });
    // Dragged far away it stops at the edge of the image.
    const far = panBy(zoomed, { x: 1e6, y: 1e6 }, stage, big);
    expect(far.offset.x).toBeCloseTo((2048 - 1000) / 2, 3);
    expect(far.offset.y).toBeCloseTo((1365 - 600) / 2, 3);
  });

  it('shows 100% as one proxy pixel per screen pixel', () => {
    const view = actualSize(FIT_VIEW, stage, big);
    expect(view.fit).toBe(false);
    expect(view.zoom).toBe(1);
    // When the stage is big enough that the fit zoom is above 100%, 100% is the fit view.
    expect(actualSize(FIT_VIEW, { width: 12000, height: 9000 }, big)).toEqual(FIT_VIEW);
  });
});
