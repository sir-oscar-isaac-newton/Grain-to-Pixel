import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';
import { useTranslation } from 'react-i18next';
import {
  settingsGet,
  settingsSet,
  type FilmFormat,
  type GuideGrid,
  type LiveViewGuides,
} from '../../../shared/ipc';
import { messageOf } from '../../library/library-store';
import styles from '../import.module.css';
import {
  clampRect,
  defaultRect,
  dragCorner,
  gridLines,
  guideKey,
  keyRect,
  moveRect,
  readRect,
  sameRect,
  type Corner,
  type GuideRect,
  type Orientation,
} from './guides';

const CORNERS: Corner[] = ['nw', 'ne', 'sw', 'se'];

/** The picture's width over its height when its size is not known yet (the camera's 3:2). */
const DEFAULT_PICTURE_ASPECT = 3 / 2;

const OFF: LiveViewGuides = { frame_lines: false, grid: 'off', adjusted: {} };

/** What a roll tells the lines: its film format and its stored frame size, if it has one. */
export interface GuideRoll {
  format: FilmFormat;
  frameSize: { w: number | null; h: number | null } | null;
}

export interface GuidesState {
  choices: LiveViewGuides;
  orientation: Orientation;
  adjusting: boolean;
  /** The lines as drawn: adjusted, or the default for the roll's format and orientation. */
  rect: GuideRect;
  /** True when the lines were adjusted for this format and orientation. */
  isAdjusted: boolean;
  /** The size of the picture's pixels, width over height. */
  pictureAspect: number;
  error: string | null;
  setFrameLines: (on: boolean) => void;
  setGrid: (grid: GuideGrid) => void;
  setOrientation: (orientation: Orientation) => void;
  setAdjusting: (on: boolean) => void;
  /** Keeps the adjusted lines for the roll's format and the orientation. */
  adjust: (rect: GuideRect) => void;
  reset: () => void;
}

/**
 * The state of the frame lines and the grid: the choices come from the settings on this computer
 * and are saved there when they change; the orientation and the adjust mode last as long as the
 * live view is open.
 */
export function useGuides(roll: GuideRoll, natural: { w: number; h: number } | null): GuidesState {
  const [choices, setChoices] = useState<LiveViewGuides>(OFF);
  const [orientation, setOrientation] = useState<Orientation>('landscape');
  const [adjusting, setAdjusting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const touched = useRef(false);
  const current = useRef(choices);
  current.current = choices;

  useEffect(() => {
    let alive = true;
    settingsGet().then(
      (settings) => {
        const kept = settings.live_view_guides;
        if (!alive || touched.current || !kept) return;
        setChoices({
          frame_lines: kept.frame_lines,
          grid: kept.grid,
          adjusted: { ...kept.adjusted },
        });
      },
      () => {
        // Unreadable settings: the lines start off, as on a first run.
      },
    );
    return () => {
      alive = false;
    };
  }, []);

  const save = (next: LiveViewGuides) => {
    touched.current = true;
    setChoices(next);
    current.current = next;
    setError(null);
    settingsSet({ live_view_guides: next }).catch((e: unknown) => setError(messageOf(e)));
  };

  const pictureAspect = natural ? natural.w / natural.h : DEFAULT_PICTURE_ASPECT;
  const key = guideKey(roll.format, orientation);
  const stored = choices.adjusted[key];
  const kept = stored ? readRect(stored) : null;
  const rect =
    kept ??
    defaultRect({
      format: roll.format,
      frameSize: roll.frameSize,
      orientation,
      pictureAspect,
    });

  return {
    choices,
    orientation,
    adjusting,
    rect,
    isAdjusted: kept !== null,
    pictureAspect,
    error,
    setFrameLines: (on) => {
      if (!on) setAdjusting(false);
      save({ ...current.current, frame_lines: on });
    },
    setGrid: (grid) => {
      save({ ...current.current, grid });
    },
    setOrientation,
    setAdjusting,
    adjust: (next) => {
      const clamped = clampRect(next);
      const base = current.current;
      save({
        ...base,
        adjusted: {
          ...base.adjusted,
          [key]: { x: clamped.x, y: clamped.y, w: clamped.w, h: clamped.h },
        },
      });
    },
    reset: () => {
      const base = current.current;
      const rest = Object.fromEntries(Object.entries(base.adjusted).filter(([k]) => k !== key));
      save({ ...base, adjusted: rest });
    },
  };
}

/** The choices for the lines and the grid, beside the focus aids. */
export function GuideControls({ guides }: { guides: GuidesState }) {
  const { t } = useTranslation();
  const { choices } = guides;
  const lastGrid = useRef<'thirds' | 'fine'>('thirds');
  if (choices.grid !== 'off') lastGrid.current = choices.grid;
  return (
    <>
      <label>
        <input
          type="checkbox"
          checked={choices.frame_lines}
          onChange={(e) => guides.setFrameLines(e.target.checked)}
        />
        {t('import.capture.liveView.guides.frameLines')}
      </label>
      {choices.frame_lines && (
        <>
          <fieldset className={styles.captureLiveLight}>
            <legend>{t('import.capture.liveView.guides.orientation')}</legend>
            {(['landscape', 'portrait'] as const).map((choice) => (
              <label key={choice}>
                <input
                  type="radio"
                  name="live-view-guide-orientation"
                  checked={guides.orientation === choice}
                  onChange={() => guides.setOrientation(choice)}
                />
                {t(`import.capture.liveView.guides.${choice}`)}
              </label>
            ))}
          </fieldset>
          <button
            type="button"
            aria-pressed={guides.adjusting}
            onClick={() => guides.setAdjusting(!guides.adjusting)}
          >
            {t('import.capture.liveView.guides.adjust')}
          </button>
          <button type="button" disabled={!guides.isAdjusted} onClick={guides.reset}>
            {t('import.capture.liveView.guides.reset')}
          </button>
        </>
      )}
      <label>
        <input
          type="checkbox"
          checked={choices.grid !== 'off'}
          onChange={(e) => guides.setGrid(e.target.checked ? lastGrid.current : 'off')}
        />
        {t('import.capture.liveView.guides.grid')}
      </label>
      {choices.grid !== 'off' && (
        <fieldset className={styles.captureLiveLight}>
          <legend>{t('import.capture.liveView.guides.gridKind')}</legend>
          {(['thirds', 'fine'] as const).map((choice) => (
            <label key={choice}>
              <input
                type="radio"
                name="live-view-guide-grid"
                checked={choices.grid === choice}
                onChange={() => guides.setGrid(choice)}
              />
              {t(`import.capture.liveView.guides.${choice}`)}
            </label>
          ))}
        </fieldset>
      )}
      {guides.error && <p role="alert">{guides.error}</p>}
    </>
  );
}

const percent = (fraction: number) => `${fraction * 100}%`;

/** A line drawn twice, light over a dark edge, so it reads on dark and bright film. */
function Line(props: { x1: number; y1: number; x2: number; y2: number }) {
  return (
    <>
      <line {...props} className={styles.captureLiveGuideEdge} />
      <line {...props} className={styles.captureLiveGuideLine} />
    </>
  );
}

/**
 * The frame lines and the grid over the picture, in fractions of the picture (the wrapper has the
 * picture's shape and is centred in the frame). A drawing only: outside the adjust mode nothing on it
 * takes the pointer, so aiming the zoom, drawing the calibration area and focus peaking work as
 * before. In the adjust mode only the lines themselves and their corners take it; the rest of the
 * picture still reaches the picture button under them.
 */
export function GuideOverlay({ guides }: { guides: GuidesState }) {
  const { t } = useTranslation();
  const { choices, rect } = guides;
  const box = useRef<HTMLDivElement>(null);
  // The lines while a drag runs; they are kept when the pointer is released.
  const [draft, setDraft] = useState<GuideRect | null>(null);
  const drag = useRef<{ x: number; y: number; start: GuideRect; corner: Corner | null } | null>(
    null,
  );
  const shown = draft ?? rect;
  const adjusting = guides.adjusting && choices.frame_lines;

  const point = (e: PointerEvent) => {
    const bounds = box.current?.getBoundingClientRect();
    if (!bounds || bounds.width === 0 || bounds.height === 0) return null;
    return {
      x: (e.clientX - bounds.left) / bounds.width,
      y: (e.clientY - bounds.top) / bounds.height,
    };
  };

  const down = (corner: Corner | null) => (e: PointerEvent<HTMLElement>) => {
    const at = point(e);
    if (!at) return;
    // The press is the lines' own: it never reaches the picture button's drag or aim.
    e.stopPropagation();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    drag.current = { x: at.x, y: at.y, start: shown, corner };
  };
  const move = (e: PointerEvent<HTMLElement>) => {
    const started = drag.current;
    const at = point(e);
    if (!started || !at) return;
    e.stopPropagation();
    setDraft(
      started.corner
        ? dragCorner(started.start, started.corner, at)
        : moveRect(started.start, at.x - started.x, at.y - started.y),
    );
  };
  const up = (e: PointerEvent<HTMLElement>) => {
    e.stopPropagation();
    drag.current = null;
    if (draft && !sameRect(draft, rect)) guides.adjust(draft);
    setDraft(null);
  };
  const cancel = () => {
    drag.current = null;
    setDraft(null);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    const next = keyRect(rect, e.key, e.shiftKey);
    if (!next) return;
    e.preventDefault();
    e.stopPropagation();
    guides.adjust(next);
  };

  const lines = choices.grid === 'off' ? [] : gridLines(choices.grid);
  return (
    <div
      ref={box}
      className={styles.captureLiveGuides}
      style={{ ['--picture-ratio' as string]: String(guides.pictureAspect) }}
    >
      <svg
        className={styles.captureLiveGuidesDrawing}
        viewBox="0 0 100 100"
        preserveAspectRatio="none"
        aria-hidden="true"
        data-testid="live-view-guides-drawing"
      >
        {choices.grid !== 'off' && (
          <g data-testid="live-view-grid" data-kind={choices.grid}>
            {lines.map((at) => (
              <Line key={`v${at}`} x1={at * 100} y1={0} x2={at * 100} y2={100} />
            ))}
            {lines.map((at) => (
              <Line key={`h${at}`} x1={0} y1={at * 100} x2={100} y2={at * 100} />
            ))}
            <Line x1={50} y1={0} x2={50} y2={100} />
            <Line x1={0} y1={50} x2={100} y2={50} />
          </g>
        )}
      </svg>
      {choices.frame_lines && (
        <div
          className={`${styles.captureLiveFrameLines} ${adjusting ? styles.captureLiveFrameLinesAdjust : ''}`}
          data-testid="live-view-frame-lines"
          role={adjusting ? 'group' : undefined}
          aria-label={adjusting ? t('import.capture.liveView.guides.adjustLabel') : undefined}
          tabIndex={adjusting ? 0 : undefined}
          aria-hidden={adjusting ? undefined : true}
          style={{
            left: percent(shown.x),
            top: percent(shown.y),
            width: percent(shown.w),
            height: percent(shown.h),
          }}
          onKeyDown={adjusting ? onKeyDown : undefined}
          onPointerDown={adjusting ? down(null) : undefined}
          onPointerMove={adjusting ? move : undefined}
          onPointerUp={adjusting ? up : undefined}
          onPointerCancel={adjusting ? cancel : undefined}
        >
          {adjusting &&
            CORNERS.map((corner) => (
              <span
                key={corner}
                className={styles.captureLiveHandle}
                data-corner={corner}
                data-testid={`live-view-handle-${corner}`}
                aria-hidden="true"
                onPointerDown={down(corner)}
                onPointerMove={move}
                onPointerUp={up}
                onPointerCancel={cancel}
              />
            ))}
        </div>
      )}
    </div>
  );
}
