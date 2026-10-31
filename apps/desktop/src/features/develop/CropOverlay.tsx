import { useRef, type CSSProperties, type KeyboardEvent, type PointerEvent } from 'react';
import styles from './develop.module.css';
import { moveCrop, nudgeCrop, resizeByHandle, type CropRect, type Handle } from './crop';

const HANDLES: readonly Handle[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'];

interface CropOverlayProps {
  crop: CropRect;
  /** Where the picture is on the stage (see `pictureStyle`), so the overlay lines up with it. */
  style: CSSProperties;
  disabled: boolean;
  /** Names the crop area for assistive technology and says how to move it. */
  label: string;
  /** The crop changed; `dragging` while it is being dragged with the pointer. */
  onChange: (crop: CropRect, options: { dragging: boolean }) => void;
}

/** What a drag in progress started from: the whole crop (move) or one of its handles. */
interface Drag {
  grip: 'move' | Handle;
  crop: CropRect;
  /** Where the pointer was when it went down, as fractions of the picture. */
  from: { x: number; y: number };
  latest: CropRect;
}

/** A fraction as a CSS percentage, to a thousandth of a percent (no float noise in the style). */
const percent = (value: number) => `${Number((value * 100).toFixed(3))}%`;

/**
 * The crop drawn over the whole scan: the rest of the picture is dimmed. The crop is dragged to
 * move it and by its sides and corners to resize it (pointer, pen or touch), and the arrow keys
 * move it when it has focus (0.5%, or 5% with Shift). The handles are for the pointer only: the
 * sides can also be set with the sliders of the Geometry panel.
 */
export function CropOverlay({ crop, style, disabled, label, onChange }: CropOverlayProps) {
  const layer = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);

  const unit = (event: PointerEvent): { x: number; y: number } | null => {
    const rect = layer.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0 || rect.height <= 0) return null;
    return {
      x: (event.clientX - rect.left) / rect.width,
      y: (event.clientY - rect.top) / rect.height,
    };
  };

  const down = (event: PointerEvent<HTMLElement>, grip: 'move' | Handle) => {
    if (disabled || event.button !== 0) return;
    const from = unit(event);
    if (!from) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    drag.current = { grip, crop, from, latest: crop };
  };

  const move = (event: PointerEvent) => {
    const current = drag.current;
    const at = current && unit(event);
    if (!current || !at) return;
    const next =
      current.grip === 'move'
        ? moveCrop(current.crop, at.x - current.from.x, at.y - current.from.y)
        : resizeByHandle(current.crop, current.grip, at);
    current.latest = next;
    onChange(next, { dragging: true });
  };

  const up = () => {
    const current = drag.current;
    drag.current = null;
    if (current) onChange(current.latest, { dragging: false });
  };

  const key = (event: KeyboardEvent<HTMLElement>) => {
    if (disabled || event.ctrlKey || event.metaKey || event.altKey) return;
    const next = nudgeCrop(crop, event.key, event.shiftKey);
    if (!next) return;
    event.preventDefault();
    event.stopPropagation();
    onChange(next, { dragging: false });
  };

  return (
    <div
      ref={layer}
      className={styles.cropLayer}
      style={style}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
    >
      {/* A rectangle moved with the pointer and the arrow keys has no standard widget role; the
          application role is the one that says the element handles those keys itself. */}
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- see above */}
      <div
        className={`${styles.crop} ${disabled ? '' : styles.cropMovable}`}
        style={{
          left: percent(crop.x),
          top: percent(crop.y),
          width: percent(crop.w),
          height: percent(crop.h),
        }}
        role="application"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        onKeyDown={key}
        onPointerDown={(event) => down(event, 'move')}
      >
        {HANDLES.map((handle) => (
          <span
            key={handle}
            className={`${styles.handle} ${styles[`handle_${handle}`]}`}
            aria-hidden="true"
            data-handle={handle}
            onPointerDown={(event) => down(event, handle)}
          />
        ))}
      </div>
    </div>
  );
}
