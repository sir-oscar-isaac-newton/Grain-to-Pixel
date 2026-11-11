// Which part of the full-size picture the detail view asks for, as plain functions.
//
// The picture is drawn at `zoom` on a stage; the part of it that shows is the visible part. The
// detail view asks for that part plus a margin, so panning a little needs no new request.

import type { Detail, DetailRegion } from '../../shared/ipc';
import { referenceSize, type Size, type View } from './viewport';

/** The visible part grows by this share of its size on each side. */
export const DETAIL_MARGIN = 0.5;
/**
 * The most pixels a request spans on a side. The backend refuses more than 4096; the fractions
 * travel as 32-bit floats and are rounded outwards to whole pixels, so this keeps some room.
 */
export const DETAIL_MAX_EDGE = 4000;
/** The first request, which only tells the size of the full picture: its centre 1%. */
export const PROBE_REGION: DetailRegion = { x: 0.495, y: 0.495, w: 0.01, h: 0.01 };

/** The part of the picture that shows on the stage, as fractions of the picture; null if none. */
export function visiblePart(view: View, stage: Size, picture: Size): DetailRegion | null {
  const reference = referenceSize(picture);
  const width = reference.width * view.zoom;
  const height = reference.height * view.zoom;
  if (width <= 0 || height <= 0 || stage.width <= 0 || stage.height <= 0) return null;
  const axis = (room: number, offset: number, shown: number) => {
    const from = Math.max(0, (-room / 2 - offset + shown / 2) / shown);
    const to = Math.min(1, (room / 2 - offset + shown / 2) / shown);
    return [from, to - from] as const;
  };
  const [x, w] = axis(stage.width, view.offset.x, width);
  const [y, h] = axis(stage.height, view.offset.y, height);
  return w > 0 && h > 0 ? { x, y, w, h } : null;
}

/**
 * The region to ask for: `visible` grown by {@link DETAIL_MARGIN} on each side, kept inside the
 * picture of `full` pixels and at most {@link DETAIL_MAX_EDGE} pixels a side (the cap keeps the
 * visible part's centre). Whole pixels, given back as fractions.
 */
export function requestRegion(visible: DetailRegion, full: Size): DetailRegion {
  const axis = (origin: number, size: number, whole: number) => {
    const length = Math.min(size * whole * (1 + 2 * DETAIL_MARGIN), DETAIL_MAX_EDGE, whole);
    const centre = (origin + size / 2) * whole;
    const first = Math.floor(Math.min(Math.max(0, centre - length / 2), whole - length));
    const last = Math.min(whole, Math.ceil(first + length));
    return [first / whole, (last - first) / whole] as const;
  };
  const [x, w] = axis(visible.x, visible.w, full.width);
  const [y, h] = axis(visible.y, visible.h, full.height);
  return { x, y, w, h };
}

/** The part of the picture a detail covers, as fractions. */
export function coveredPart(detail: Detail): DetailRegion {
  return {
    x: detail.x / detail.fullWidth,
    y: detail.y / detail.fullHeight,
    w: detail.width / detail.fullWidth,
    h: detail.height / detail.fullHeight,
  };
}

/** Whether `inner` lies inside `outer` (to a hair, for rounding). */
export function contains(outer: DetailRegion, inner: DetailRegion): boolean {
  const slack = 1e-9;
  return (
    inner.x >= outer.x - slack &&
    inner.y >= outer.y - slack &&
    inner.x + inner.w <= outer.x + outer.w + slack &&
    inner.y + inner.h <= outer.y + outer.h + slack
  );
}
