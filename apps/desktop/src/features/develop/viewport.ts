// Pan and zoom of the Develop canvas, as plain functions.
//
// A preview comes in two sizes (1600 px while a slider moves, 2048 px after), so the zoom is not
// measured in preview pixels. It is measured against the size the image has at the proxy's long
// edge (`PROXY_EDGE`): at zoom 1 (100%) one such pixel is one screen pixel, and a preview of
// either size fills the same area on screen.

export const PROXY_EDGE = 2048;
/** The largest zoom: 400% of the proxy. */
export const MAX_ZOOM = 4;
/** One wheel notch (or a zoom button) multiplies the zoom by this. */
export const ZOOM_STEP = 1.25;

export interface Size {
  width: number;
  height: number;
}

export interface Point {
  x: number;
  y: number;
}

/** Where the image is: `zoom`, and `offset`, how far its centre is from the centre of the stage. */
export interface View {
  /** True until the user zooms or pans: the image then follows the size of the stage. */
  fit: boolean;
  zoom: number;
  offset: Point;
}

export const FIT_VIEW: View = { fit: true, zoom: 1, offset: { x: 0, y: 0 } };

/** The size an image has at the proxy's long edge, the unit of the zoom. */
export function referenceSize(image: Size): Size {
  const long = Math.max(image.width, image.height);
  if (long <= 0) return { width: 0, height: 0 };
  const scale = PROXY_EDGE / long;
  return { width: image.width * scale, height: image.height * scale };
}

/** The zoom at which the whole image just fits the stage (empty stages and images give 1). */
export function fitZoom(stage: Size, image: Size): number {
  const reference = referenceSize(image);
  if (stage.width <= 0 || stage.height <= 0 || reference.width <= 0) return 1;
  return Math.min(stage.width / reference.width, stage.height / reference.height);
}

/** The smallest and largest zoom: the fit zoom, and 400% (or the fit zoom, if that is larger). */
export function zoomLimits(stage: Size, image: Size): { min: number; max: number } {
  const fit = fitZoom(stage, image);
  return { min: fit, max: Math.max(MAX_ZOOM, fit) };
}

/** Keeps the image from being dragged away: it stays centred along an axis it does not fill. */
export function clampOffset(offset: Point, zoom: number, stage: Size, image: Size): Point {
  const reference = referenceSize(image);
  const limit = (shown: number, room: number) => Math.max(0, (shown - room) / 2);
  const x = limit(reference.width * zoom, stage.width);
  const y = limit(reference.height * zoom, stage.height);
  // + 0 turns a -0 into 0.
  return {
    x: Math.min(x, Math.max(-x, offset.x)) + 0,
    y: Math.min(y, Math.max(-y, offset.y)) + 0,
  };
}

/** The view as it is drawn: a fitting view follows the stage, any other is clamped to it. */
export function resolveView(view: View, stage: Size, image: Size): View {
  if (view.fit) return { fit: true, zoom: fitZoom(stage, image), offset: { x: 0, y: 0 } };
  const { min, max } = zoomLimits(stage, image);
  const zoom = Math.min(max, Math.max(min, view.zoom));
  return { fit: false, zoom, offset: clampOffset(view.offset, zoom, stage, image) };
}

/**
 * Zooms to `target`, keeping the image point under `anchor` (a point relative to the centre of the
 * stage) where it is. Reaching the fit zoom goes back to a fitting view.
 */
export function zoomTo(view: View, target: number, anchor: Point, stage: Size, image: Size): View {
  const current = resolveView(view, stage, image);
  const { min, max } = zoomLimits(stage, image);
  const zoom = Math.min(max, Math.max(min, target));
  if (zoom <= min + 1e-9) return FIT_VIEW;
  const ratio = zoom / current.zoom;
  const offset = {
    x: anchor.x - (anchor.x - current.offset.x) * ratio,
    y: anchor.y - (anchor.y - current.offset.y) * ratio,
  };
  return { fit: false, zoom, offset: clampOffset(offset, zoom, stage, image) };
}

/** Moves the image by `delta` screen pixels. A fitting view does not move. */
export function panBy(view: View, delta: Point, stage: Size, image: Size): View {
  if (view.fit) return view;
  const current = resolveView(view, stage, image);
  return {
    ...current,
    offset: clampOffset(
      { x: current.offset.x + delta.x, y: current.offset.y + delta.y },
      current.zoom,
      stage,
      image,
    ),
  };
}

/** The view at 100% (one proxy pixel per screen pixel), or a fitting view when that is as big. */
export function actualSize(view: View, stage: Size, image: Size): View {
  return zoomTo(view, 1, { x: 0, y: 0 }, stage, image);
}
