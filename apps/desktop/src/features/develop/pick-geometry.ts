// Picking on a picture that is shown turned, flipped and straightened: the backend takes regions
// of the frame before those steps, so a region picked on the shown picture is mapped back here.
// This is the inverse of the forward transform in crates/gtp-edit/src/geometry.rs: crop, straighten
// around the crop centre (clockwise for a positive angle, output the largest rectangle with the
// crop's aspect ratio inside the rotated crop), quarter turns clockwise, then flips.
import type { Fractions } from './picker';

/** The parts of a recipe's geometry that change how the picture is shown. */
export interface PickGeometry {
  crop: Fractions;
  angle_deg: number;
  rot90: number;
  flip_h: boolean;
  flip_v: boolean;
}

/** The size of the scan in pixels. */
export interface ScanSize {
  width: number;
  height: number;
}

interface Pt {
  x: number;
  y: number;
}

const clamp01 = (value: number) => Math.min(1, Math.max(0, value));

/** Rounds a crop edge to whole pixels, as the backend does. */
const edge = (fraction: number, size: number) =>
  Math.min(size, Math.max(0, Math.round(fraction * size)));

/** The crop in whole source pixels, as the backend rounds it. */
function cropPixels(crop: Fractions, scan: ScanSize) {
  const x0 = edge(crop.x, scan.width);
  const y0 = edge(crop.y, scan.height);
  const x1 = Math.max(x0, edge(crop.x + crop.w, scan.width));
  const y1 = Math.max(y0, edge(crop.y + crop.h, scan.height));
  return { w: x1 - x0, h: y1 - y0 };
}

/** The size of the straightened picture in pixels (before turns), as the backend plans it. */
export function straightSize(w: number, h: number, angleDeg: number): { w: number; h: number } {
  if (angleDeg === 0) return { w, h };
  const theta = (angleDeg * Math.PI) / 180;
  const sin = Math.sin(theta);
  const cos = Math.cos(theta);
  const a = w / 2;
  const b = h / 2;
  const fitX = a / (a * Math.abs(cos) + b * Math.abs(sin));
  const fitY = b / (a * Math.abs(sin) + b * Math.abs(cos));
  const scale = Math.min(fitX, fitY, 1);
  const shrink = (size: number) => Math.max(1, Math.floor(scale * size + 1e-6));
  return { w: shrink(w), h: shrink(h) };
}

/**
 * Where a point of the shown picture (fractions of it) is in the cropped frame before straightening
 * (fractions of the crop): the flips are undone first, then the turn, then the straightening.
 */
export function pointToFrame(point: Pt, geometry: PickGeometry, scan: ScanSize): Pt {
  const crop = cropPixels(geometry.crop, scan);
  let { x, y } = point;
  // Flips act in the final orientation.
  if (geometry.flip_h) x = 1 - x;
  if (geometry.flip_v) y = 1 - y;
  // The turn: undo `rot90` quarter turns clockwise, giving a point of the straightened picture.
  const turns = ((Math.round(geometry.rot90) % 4) + 4) % 4;
  if (turns === 1) [x, y] = [y, 1 - x];
  else if (turns === 2) [x, y] = [1 - x, 1 - y];
  else if (turns === 3) [x, y] = [1 - y, x];
  if (geometry.angle_deg === 0 || crop.w === 0 || crop.h === 0) return { x, y };
  // The straightening is a rotation only when both axes are in pixels, so go through pixels.
  const straight = straightSize(crop.w, crop.h, geometry.angle_deg);
  const theta = (geometry.angle_deg * Math.PI) / 180;
  const sin = Math.sin(theta);
  const cos = Math.cos(theta);
  const u = (x - 0.5) * straight.w;
  const v = (y - 0.5) * straight.h;
  return {
    x: (crop.w / 2 + u * cos + v * sin) / crop.w,
    y: (crop.h / 2 - u * sin + v * cos) / crop.h,
  };
}

/**
 * A region of the shown picture (fractions of it) as a region of the cropped frame before
 * straightening, turns and flips (fractions of the crop), which is what the sample commands take.
 * Turns and flips are exact; with an angle the four corners are mapped and the result is the
 * smallest upright box around them. Kept inside the frame.
 */
export function regionToFrame(
  region: Fractions,
  geometry: PickGeometry,
  scan: ScanSize,
): Fractions {
  const corners = [
    { x: region.x, y: region.y },
    { x: region.x + region.w, y: region.y },
    { x: region.x, y: region.y + region.h },
    { x: region.x + region.w, y: region.y + region.h },
  ].map((corner) => pointToFrame(corner, geometry, scan));
  const xs = corners.map((c) => clamp01(c.x));
  const ys = corners.map((c) => clamp01(c.y));
  const x = Math.min(...xs);
  const y = Math.min(...ys);
  return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y };
}

/**
 * A region picked on the whole scan shown turned and flipped like the frame (no crop, no angle),
 * as a region of the scan as it was scanned.
 */
export function regionToScan(
  region: Fractions,
  geometry: Pick<PickGeometry, 'rot90' | 'flip_h' | 'flip_v'>,
  scan: ScanSize,
): Fractions {
  const whole = { x: 0, y: 0, w: 1, h: 1 };
  return regionToFrame(region, { ...geometry, crop: whole, angle_deg: 0 }, scan);
}
