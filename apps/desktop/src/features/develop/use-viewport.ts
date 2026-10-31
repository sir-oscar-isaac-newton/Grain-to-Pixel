import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from 'react';
import {
  FIT_VIEW,
  ZOOM_STEP,
  actualSize,
  panBy,
  resolveView,
  zoomTo,
  type Point,
  type Size,
  type View,
} from './viewport';

export interface Viewport {
  /** Attach to the stage element: its size is the room the image has. */
  stageRef: RefObject<HTMLDivElement>;
  /** The view as drawn (a fitting view follows the stage). */
  view: View;
  /** True when the image is at the fit zoom (nothing to pan). */
  fitting: boolean;
  fit: () => void;
  actual: () => void;
  zoomIn: () => void;
  zoomOut: () => void;
}

/**
 * Pan and zoom of an image of `image` size on a stage: wheel to zoom around the cursor, drag to
 * pan, and commands for the buttons and keys. The zoom is kept when the preview changes size (see
 * `viewport.ts`), so a sharper preview replaces a rougher one without the picture moving.
 */
export function useViewport(image: Size | null): Viewport {
  const stageRef = useRef<HTMLDivElement>(null);
  const [stage, setStage] = useState<Size>({ width: 0, height: 0 });
  const [view, setView] = useState<View>(FIT_VIEW);
  const size = useMemo<Size>(
    () => ({ width: image?.width ?? 0, height: image?.height ?? 0 }),
    [image?.width, image?.height],
  );

  useLayoutEffect(() => {
    const element = stageRef.current;
    if (!element) return;
    const read = () => setStage({ width: element.clientWidth, height: element.clientHeight });
    read();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(read);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  // Latest values for the listeners below, which are attached once.
  const live = useRef({ view, stage, size });
  live.current = { view, stage, size };

  const resolved = useMemo(() => resolveView(view, stage, size), [view, stage, size]);

  const zoomBy = useCallback((factor: number, anchor?: Point) => {
    const { view: v, stage: s, size: i } = live.current;
    const current = resolveView(v, s, i);
    setView(zoomTo(v, current.zoom * factor, anchor ?? { x: 0, y: 0 }, s, i));
  }, []);

  // The wheel zooms around the cursor. It needs a listener that may cancel the page scroll, which
  // React's own wheel handlers cannot do.
  useEffect(() => {
    const element = stageRef.current;
    if (!element) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const box = element.getBoundingClientRect();
      const anchor = {
        x: event.clientX - box.left - box.width / 2,
        y: event.clientY - box.top - box.height / 2,
      };
      zoomBy(event.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP, anchor);
    };
    element.addEventListener('wheel', onWheel, { passive: false });

    // Dragging pans a zoomed image. (Keys and buttons do everything else, so the stage itself needs
    // no keyboard handling.)
    const onMouseDown = (event: MouseEvent) => {
      if (event.button !== 0) return;
      const { view: v, stage: s, size: i } = live.current;
      if (resolveView(v, s, i).fit) return;
      event.preventDefault();
      let last = { x: event.clientX, y: event.clientY };
      const move = (e: MouseEvent) => {
        const delta = { x: e.clientX - last.x, y: e.clientY - last.y };
        last = { x: e.clientX, y: e.clientY };
        const now = live.current;
        setView(panBy(now.view, delta, now.stage, now.size));
      };
      const stop = () => {
        window.removeEventListener('mousemove', move);
        window.removeEventListener('mouseup', stop);
      };
      window.addEventListener('mousemove', move);
      window.addEventListener('mouseup', stop);
    };
    element.addEventListener('mousedown', onMouseDown);
    return () => {
      element.removeEventListener('wheel', onWheel);
      element.removeEventListener('mousedown', onMouseDown);
    };
  }, [zoomBy]);

  const fit = useCallback(() => setView(FIT_VIEW), []);
  const actual = useCallback(() => {
    const { view: v, stage: s, size: i } = live.current;
    setView(actualSize(v, s, i));
  }, []);
  const zoomIn = useCallback(() => zoomBy(ZOOM_STEP), [zoomBy]);
  const zoomOut = useCallback(() => zoomBy(1 / ZOOM_STEP), [zoomBy]);

  return {
    stageRef,
    view: resolved,
    fitting: resolved.fit,
    fit,
    actual,
    zoomIn,
    zoomOut,
  };
}
