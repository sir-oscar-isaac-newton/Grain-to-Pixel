import { useEffect, useRef, useState, type RefObject } from 'react';
import {
  DRAG_THRESHOLD_PX,
  boxRegion,
  clickRegion,
  distance,
  type Box,
  type Fractions,
} from './picker';

interface PickerOptions {
  /** The stage the picture is drawn on. */
  stageRef: RefObject<HTMLDivElement>;
  /** Picking is on: presses on the picture pick instead of doing nothing. */
  active: boolean;
  /** Called with the region picked, in fractions of the picture. */
  onPick: (region: Fractions) => void;
}

/**
 * Picking on the picture: a click covers a small square around it, a dragged box covers the box.
 * The picture must show the whole scan while this is on, so that its fractions are the scan's.
 * Returns the box being dragged, in pixels inside the stage, for drawing.
 */
export function usePicker({ stageRef, active, onPick }: PickerOptions): Box | null {
  const [box, setBox] = useState<Box | null>(null);
  const pick = useRef(onPick);
  pick.current = onPick;

  useEffect(() => {
    const stage = stageRef.current;
    if (!active || !stage) return;
    // Ends a drag that is under way without picking anything.
    let cancelDrag: (() => void) | null = null;

    const onMouseDown = (event: MouseEvent) => {
      if (event.button !== 0) return;
      const canvas = stage.querySelector('canvas');
      if (!canvas) return;
      const picture = canvas.getBoundingClientRect();
      const start = { x: event.clientX, y: event.clientY };
      // A press outside the picture picks nothing and does not start a box.
      if (!clickRegion(start, picture)) return;
      event.preventDefault();
      const room = stage.getBoundingClientRect();

      const move = (e: MouseEvent) => {
        const here = { x: e.clientX, y: e.clientY };
        if (distance(start, here) < DRAG_THRESHOLD_PX) {
          setBox(null);
          return;
        }
        setBox({
          left: Math.min(start.x, here.x) - room.left,
          top: Math.min(start.y, here.y) - room.top,
          width: Math.abs(here.x - start.x),
          height: Math.abs(here.y - start.y),
        });
      };
      const finish = () => {
        window.removeEventListener('mousemove', move);
        window.removeEventListener('mouseup', stop);
        cancelDrag = null;
        setBox(null);
      };
      const stop = (e: MouseEvent) => {
        finish();
        const end = { x: e.clientX, y: e.clientY };
        const region =
          distance(start, end) < DRAG_THRESHOLD_PX
            ? clickRegion(start, picture)
            : boxRegion(start, end, picture);
        if (region) pick.current(region);
      };
      window.addEventListener('mousemove', move);
      window.addEventListener('mouseup', stop);
      cancelDrag = finish;
    };

    stage.addEventListener('mousedown', onMouseDown);
    return () => {
      stage.removeEventListener('mousedown', onMouseDown);
      cancelDrag?.();
      setBox(null);
    };
  }, [active, stageRef]);

  return box;
}
