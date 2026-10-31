import { describe, expect, it } from 'vitest';
import {
  BIG_NUDGE,
  FULL_CROP,
  MIN_CROP,
  NUDGE,
  clampCrop,
  edgesOf,
  moveCrop,
  moveEdge,
  nudgeCrop,
  resizeByHandle,
  sameCrop,
  type CropRect,
} from './crop';

const crop: CropRect = { x: 0.2, y: 0.1, w: 0.5, h: 0.6 };

describe('clampCrop', () => {
  it('keeps a good crop as it is', () => {
    expect(clampCrop(crop)).toEqual(crop);
    expect(clampCrop(FULL_CROP)).toEqual(FULL_CROP);
  });

  it('pulls a crop that sticks out back inside the scan', () => {
    expect(clampCrop({ x: 0.8, y: -0.2, w: 0.5, h: 0.4 })).toEqual({
      x: 0.5,
      y: 0,
      w: 0.5,
      h: 0.4,
    });
    expect(clampCrop({ x: 0, y: 0, w: 4, h: 9 })).toEqual(FULL_CROP);
  });

  it('keeps at least the smallest size, and survives numbers that are not numbers', () => {
    expect(clampCrop({ x: 0.3, y: 0.3, w: 0, h: -1 })).toEqual({
      x: 0.3,
      y: 0.3,
      w: MIN_CROP,
      h: MIN_CROP,
    });
    expect(
      clampCrop({ x: Number.NaN, y: Number.POSITIVE_INFINITY, w: Number.NaN, h: Number.NaN }),
    ).toEqual({ x: 0, y: 0, w: 1, h: 1 });
  });
});

describe('moveCrop', () => {
  it('moves the crop without changing its size', () => {
    const moved = moveCrop(crop, 0.1, -0.05);
    expect(moved.x).toBeCloseTo(0.3, 9);
    expect(moved.y).toBeCloseTo(0.05, 9);
    expect([moved.w, moved.h]).toEqual([0.5, 0.6]);
  });

  it('stops at the edges of the scan', () => {
    expect(moveCrop(crop, 5, 5)).toEqual({ x: 0.5, y: 0.4, w: 0.5, h: 0.6 });
    expect(moveCrop(crop, -5, -5)).toEqual({ x: 0, y: 0, w: 0.5, h: 0.6 });
    expect(moveCrop(FULL_CROP, 0.2, 0.2)).toEqual(FULL_CROP);
  });
});

describe('moveEdge', () => {
  it('sets one side and leaves the others', () => {
    const edges = edgesOf(moveEdge(crop, 'left', 0.1));
    expect(edges.left).toBeCloseTo(0.1, 9);
    expect(edges.right).toBeCloseTo(0.7, 9);
    expect(edges.top).toBeCloseTo(0.1, 9);
    expect(edges.bottom).toBeCloseTo(0.7, 9);
    expect(edgesOf(moveEdge(crop, 'bottom', 0.9)).bottom).toBeCloseTo(0.9, 9);
    expect(edgesOf(moveEdge(crop, 'top', 0.3)).top).toBeCloseTo(0.3, 9);
    expect(edgesOf(moveEdge(crop, 'right', 0.95)).right).toBeCloseTo(0.95, 9);
  });

  it('stops at the scan and at the smallest size from the opposite side', () => {
    expect(edgesOf(moveEdge(crop, 'left', -3)).left).toBe(0);
    expect(edgesOf(moveEdge(crop, 'right', 3)).right).toBe(1);
    expect(edgesOf(moveEdge(crop, 'top', -3)).top).toBe(0);
    expect(edgesOf(moveEdge(crop, 'bottom', 3)).bottom).toBe(1);
    const squeezed = moveEdge(crop, 'left', 5);
    expect(squeezed.w).toBeCloseTo(MIN_CROP, 9);
    expect(edgesOf(squeezed).right).toBeCloseTo(0.7, 9);
    expect(moveEdge(crop, 'bottom', -5).h).toBeCloseTo(MIN_CROP, 9);
    expect(sameCrop(moveEdge(crop, 'right', Number.NaN), crop)).toBe(true);
  });
});

describe('resizeByHandle', () => {
  const at = (x: number, y: number) => ({ x, y });

  it('moves the sides a handle is on', () => {
    expect(resizeByHandle(crop, 'e', at(0.9, 0.5))).toMatchObject({
      x: 0.2,
      w: expect.closeTo(0.7, 9),
    });
    expect(resizeByHandle(crop, 'n', at(0.9, 0.3)).x).toBe(0.2);
    expect(edgesOf(resizeByHandle(crop, 'n', at(0.9, 0.3))).top).toBeCloseTo(0.3, 9);
    const corner = edgesOf(resizeByHandle(crop, 'se', at(0.8, 0.9)));
    expect(corner.right).toBeCloseTo(0.8, 9);
    expect(corner.bottom).toBeCloseTo(0.9, 9);
    expect(corner.left).toBeCloseTo(0.2, 9);
    expect(corner.top).toBeCloseTo(0.1, 9);
    const other = edgesOf(resizeByHandle(crop, 'nw', at(0.05, 0.02)));
    expect([other.left, other.top]).toEqual([0.05, 0.02]);
    expect(edgesOf(resizeByHandle(crop, 'sw', at(0.1, 0.8))).left).toBeCloseTo(0.1, 9);
    expect(edgesOf(resizeByHandle(crop, 'ne', at(0.9, 0.05))).right).toBeCloseTo(0.9, 9);
    expect(edgesOf(resizeByHandle(crop, 'w', at(0.1, 0.5))).left).toBeCloseTo(0.1, 9);
    expect(edgesOf(resizeByHandle(crop, 's', at(0.5, 0.95))).bottom).toBeCloseTo(0.95, 9);
  });

  it('cannot drag a side past the opposite one', () => {
    const dragged = resizeByHandle(crop, 'se', at(0, 0));
    expect(dragged.w).toBeCloseTo(MIN_CROP, 9);
    expect(dragged.h).toBeCloseTo(MIN_CROP, 9);
    expect([dragged.x, dragged.y]).toEqual([0.2, 0.1]);
  });
});

describe('nudgeCrop', () => {
  it('moves one small step per arrow key, a big one with Shift', () => {
    expect(nudgeCrop(crop, 'ArrowRight', false)?.x).toBeCloseTo(0.2 + NUDGE, 9);
    expect(nudgeCrop(crop, 'ArrowLeft', false)?.x).toBeCloseTo(0.2 - NUDGE, 9);
    expect(nudgeCrop(crop, 'ArrowDown', false)?.y).toBeCloseTo(0.1 + NUDGE, 9);
    expect(nudgeCrop(crop, 'ArrowUp', true)?.y).toBeCloseTo(0.1 - BIG_NUDGE, 9);
    expect(nudgeCrop(crop, 'ArrowRight', true)?.x).toBeCloseTo(0.2 + BIG_NUDGE, 9);
  });

  it('ignores any other key, and stops at the edge', () => {
    expect(nudgeCrop(crop, 'a', false)).toBeNull();
    expect(nudgeCrop(crop, 'Enter', true)).toBeNull();
    expect(nudgeCrop({ ...crop, x: 0 }, 'ArrowLeft', true)?.x).toBe(0);
  });
});

describe('sameCrop', () => {
  it('compares to what the recipe can tell apart', () => {
    expect(sameCrop(crop, { ...crop })).toBe(true);
    expect(sameCrop(crop, { ...crop, x: crop.x + 1e-9 })).toBe(true);
    expect(sameCrop(crop, { ...crop, w: 0.51 })).toBe(false);
  });
});
