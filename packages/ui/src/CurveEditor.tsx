import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';
import styles from './CurveEditor.module.css';
import {
  addPointAt,
  addPointInGap,
  curvePath,
  identityCurve,
  movePoint,
  removePoint,
  type CurvePoint,
} from './curve-math';

export type CurveChannel = 'master' | 'r' | 'g' | 'b';
export type CurveSet = Record<CurveChannel, CurvePoint[]>;

/** Everything the editor says, already translated. */
export interface CurveEditorText {
  /** Names the switch between the four curves. */
  channels: string;
  master: string;
  red: string;
  green: string;
  blue: string;
  /** Names the drawing, for example "Tone curve, Master". */
  plot: (channel: string) => string;
  /** Shown for a colour curve that has no points: it changes nothing. */
  unchanged: string;
  add: string;
  remove: string;
  reset: string;
  /** Names a point, for example "Point 2 of 3". */
  point: (n: number, total: number) => string;
  /** What a point says, for example "Input 0.50, output 0.62". */
  value: (input: string, output: string) => string;
  /** The number format for inputs and outputs. */
  number: (value: number) => string;
}

interface CurveEditorProps {
  curves: CurveSet;
  text: CurveEditorText;
  /** The curve of `channel` changed; `dragging` while a point is being dragged. */
  onChange: (channel: CurveChannel, points: CurvePoint[], options: { dragging: boolean }) => void;
  disabled?: boolean;
}

const SIZE = 256;
const PAD = 8;
const STEP = 0.01;
const BIG_STEP = 0.1;
const CHANNELS: readonly CurveChannel[] = ['master', 'r', 'g', 'b'];
const GRID = [0.25, 0.5, 0.75];

/**
 * A tone curve editor: four curves (master and the three colour channels), each a smooth curve
 * through up to 16 points. Points move with the arrow keys (0.01, or 0.1 with Shift), the pointer
 * or touch; Delete removes one, "Add point" and a double click add one.
 */
export function CurveEditor({ curves, text, onChange, disabled = false }: CurveEditorProps) {
  const [channel, setChannel] = useState<CurveChannel>('master');
  const [selected, setSelected] = useState<number | null>(null);
  const svg = useRef<SVGSVGElement | null>(null);
  const dots = useRef<Array<SVGCircleElement | null>>([]);
  const focusNext = useRef<number | null>(null);
  const dragging = useRef<{ index: number; points: CurvePoint[] } | null>(null);

  const points = curves[channel];
  const names: Record<CurveChannel, string> = {
    master: text.master,
    r: text.red,
    g: text.green,
    b: text.blue,
  };

  // After adding or removing a point, focus goes to the point the person is working on.
  useEffect(() => {
    if (focusNext.current === null) return;
    dots.current[focusNext.current]?.focus();
    focusNext.current = null;
  });

  const emit = (next: CurvePoint[], isDragging: boolean) =>
    onChange(channel, next, { dragging: isDragging });

  /** A curve with no points of its own is the identity; editing it starts from that line. */
  const base = () => (points.length === 0 ? identityCurve() : points);

  const addInGap = () => {
    const added = addPointInGap(base());
    if (!added) return;
    focusNext.current = added.index;
    setSelected(added.index);
    emit(added.points, false);
  };

  const unitAt = (clientX: number, clientY: number): [number, number] | null => {
    const rect = svg.current?.getBoundingClientRect();
    if (!rect || rect.width === 0 || rect.height === 0) return null;
    const scale = (SIZE + 2 * PAD) / rect.width;
    return [
      ((clientX - rect.left) * scale - PAD) / SIZE,
      1 - ((clientY - rect.top) * scale - PAD) / SIZE,
    ];
  };

  const onKey = (event: KeyboardEvent<SVGCircleElement>, index: number) => {
    if (disabled) return;
    const point = points[index];
    if (!point) return;
    const step = event.shiftKey ? BIG_STEP : STEP;
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, step],
      ArrowDown: [0, -step],
    };
    const move = moves[event.key];
    if (move) {
      event.preventDefault();
      emit(movePoint(points, index, point[0] + move[0], point[1] + move[1]), false);
    } else if (event.key === 'Delete' || event.key === 'Backspace') {
      event.preventDefault();
      const next = removePoint(points, index);
      if (!next) return;
      focusNext.current = Math.min(index, next.length - 1);
      setSelected(focusNext.current);
      emit(next, false);
    }
  };

  const onDown = (event: PointerEvent<SVGCircleElement>, index: number) => {
    setSelected(index);
    if (disabled || event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.focus();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    dragging.current = { index, points };
  };

  const onMove = (event: PointerEvent<SVGCircleElement>) => {
    const drag = dragging.current;
    const at = drag && unitAt(event.clientX, event.clientY);
    if (!drag || !at) return;
    const next = movePoint(drag.points, drag.index, at[0], at[1]);
    dragging.current = { index: drag.index, points: next };
    emit(next, true);
  };

  const onUp = () => {
    const drag = dragging.current;
    dragging.current = null;
    if (drag) emit(drag.points, false);
  };

  const onDoubleClick = (
    event: PointerEvent<SVGRectElement> | React.MouseEvent<SVGRectElement>,
  ) => {
    if (disabled) return;
    const at = unitAt(event.clientX, event.clientY);
    if (!at) return;
    const added = addPointAt(base(), at[0], at[1]);
    if (!added) return;
    focusNext.current = added.index;
    setSelected(added.index);
    emit(added.points, false);
  };

  const px = (v: number) => v * SIZE;
  const py = (v: number) => (1 - v) * SIZE;
  const count = points.length;

  return (
    <div className={styles.editor}>
      <div role="group" aria-label={text.channels} className={styles.channels}>
        {CHANNELS.map((c) => (
          <button
            key={c}
            type="button"
            aria-pressed={c === channel}
            className={styles[c]}
            onClick={() => {
              setChannel(c);
              setSelected(null);
            }}
          >
            {names[c]}
          </button>
        ))}
      </div>

      <svg
        ref={svg}
        className={styles.plot}
        viewBox={`${-PAD} ${-PAD} ${SIZE + 2 * PAD} ${SIZE + 2 * PAD}`}
        role="group"
        aria-label={text.plot(names[channel])}
      >
        <rect
          className={styles.field}
          x={0}
          y={0}
          width={SIZE}
          height={SIZE}
          onDoubleClick={onDoubleClick}
        />
        {GRID.map((g) => (
          <g key={g} className={styles.grid} aria-hidden="true">
            <line x1={px(g)} y1={0} x2={px(g)} y2={SIZE} />
            <line x1={0} y1={py(g)} x2={SIZE} y2={py(g)} />
          </g>
        ))}
        <line className={styles.diagonal} x1={0} y1={SIZE} x2={SIZE} y2={0} aria-hidden="true" />
        <path
          className={`${styles.curve} ${styles[`curve_${channel}`]}`}
          d={curvePath(points, SIZE)}
        />
        {points.map(([x, y], index) => (
          <circle
            // Points have no identity of their own: the position in the list is the name.
            key={index}
            ref={(el) => {
              dots.current[index] = el;
            }}
            className={`${styles.point} ${selected === index ? styles.selected : ''}`}
            cx={px(x)}
            cy={py(y)}
            r={6}
            tabIndex={disabled ? -1 : 0}
            role="slider"
            aria-label={text.point(index + 1, count)}
            aria-orientation="vertical"
            aria-valuemin={0}
            aria-valuemax={1}
            aria-valuenow={Number(y.toFixed(3))}
            aria-valuetext={text.value(text.number(x), text.number(y))}
            aria-disabled={disabled}
            onFocus={() => setSelected(index)}
            onKeyDown={(event) => onKey(event, index)}
            onPointerDown={(event) => onDown(event, index)}
            onPointerMove={onMove}
            onPointerUp={onUp}
            onPointerCancel={onUp}
          />
        ))}
      </svg>
      {count === 0 && <p className={styles.note}>{text.unchanged}</p>}

      <div className={styles.buttons}>
        <button type="button" disabled={disabled} onClick={addInGap}>
          {text.add}
        </button>
        <button
          type="button"
          disabled={disabled || selected === null || count <= 2}
          onClick={() => {
            if (selected === null) return;
            const next = removePoint(points, selected);
            if (!next) return;
            focusNext.current = Math.min(selected, next.length - 1);
            setSelected(focusNext.current);
            emit(next, false);
          }}
        >
          {text.remove}
        </button>
        <button
          type="button"
          disabled={disabled || (channel !== 'master' && count === 0)}
          onClick={() => {
            setSelected(null);
            emit(channel === 'master' ? identityCurve() : [], false);
          }}
        >
          {text.reset}
        </button>
      </div>
    </div>
  );
}
