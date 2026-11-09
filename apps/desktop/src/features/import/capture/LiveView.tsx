import { useEffect, useRef, useState, type MutableRefObject } from 'react';
import { useTranslation } from 'react-i18next';
import {
  cameraPreviewFrame,
  cameraPreviewSetArea,
  cameraPreviewSetZoom,
  cameraPreviewStop,
  cameraPreviewZoomSteps,
  lightOff,
  lightSet,
  type Levels,
  type ZoomStep,
} from '../../../shared/ipc';
import { messageOf } from '../../library/library-store';
import styles from '../import.module.css';
import { canAnalyse, createAnalyser } from './analyse-frame';
import {
  MAX_SENSITIVITY,
  MIN_SENSITIVITY,
  moved,
  nudgeStep,
  PEAK_COLOURS,
  type PeakColour,
} from './focus';

/** The frame rate is measured over this many milliseconds. */
const RATE_WINDOW_MS = 1000;

const COLOURS = ['off', 'red', 'green', 'blue', 'all'] as const;
type Colour = (typeof COLOURS)[number];

/** The levels that light one colour (or all three) at the working levels. */
function levelsFor(colour: Colour, [r, g, b]: Levels): Levels {
  switch (colour) {
    case 'red':
      return [r, 0, 0];
    case 'green':
      return [0, g, 0];
    case 'blue':
      return [0, 0, b];
    case 'all':
      return [r, g, b];
    default:
      return [0, 0, 0];
  }
}

const PEAK_CHOICES = Object.keys(PEAK_COLOURS) as PeakColour[];

/** The middle of the picture, where a zoomed spot starts and where a key press on it points. */
const MIDDLE = { x: 0.5, y: 0.5 };

const ARROWS = [
  { key: 'moveLeft', dx: -1, dy: 0 },
  { key: 'moveRight', dx: 1, dy: 0 },
  { key: 'moveUp', dx: 0, dy: -1 },
  { key: 'moveDown', dx: 0, dy: 1 },
] as const;

const oneDecimal = (value: number) =>
  value.toLocaleString(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 1 });

interface LiveViewProps {
  /** The working levels of the sliders; null until they are known. */
  levels: Levels | null;
  /** True when the light is not connected or has tripped: it is off then. */
  lightLost: boolean;
  /** True while a capture, test shot or calibration runs or starts: live view stops and cannot start. */
  blocked: boolean;
  /**
   * Set to a function that stops the loop and takes the camera out of live view (nothing happens
   * when it is not running). The section calls it first when it closes, before the light goes off:
   * a ref of the panel itself would already be detached by then.
   */
  stopRef: MutableRefObject<(() => void) | null>;
}

/** One run of the frame loop; `stopped` ends it, also for a request that is still on its way. */
interface Run {
  stopped: boolean;
}

/**
 * The camera's live view: one frame at a time, the next requested as soon as the last arrived.
 * Live view never lights the Scanlight; the light controls do that.
 */
export function LiveView({ levels, lightLost, blocked, stopRef }: LiveViewProps) {
  const { t } = useTranslation();
  const [running, setRunning] = useState(false);
  const [url, setUrl] = useState<string | null>(null);
  const [rate, setRate] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = useRef<Run | null>(null);

  // Focus aids: peaking and the sharpness number come from each frame, the zoom from the camera.
  const [peaking, setPeaking] = useState(false);
  const [sensitivity, setSensitivity] = useState(50);
  const [peakColour, setPeakColour] = useState<PeakColour>('green');
  const [sharpness, setSharpness] = useState<number | null>(null);
  const [best, setBest] = useState<number | null>(null);
  const [steps, setSteps] = useState<ZoomStep[]>([]);
  // The chosen step (0 is the whole frame) and the spot of the frame it is aimed at.
  const [zoomIndex, setZoomIndex] = useState(0);
  const [area, setArea] = useState<{ x: number; y: number } | null>(null);
  const overlay = useRef<HTMLCanvasElement>(null);
  const analyser = useRef(createAnalyser());
  const analysing = useRef(false);
  const bestRef = useRef<number | null>(null);
  const aids = useRef({ peaking, sensitivity, colour: peakColour, zoomed: zoomIndex > 0 });
  aids.current = { peaking, sensitivity, colour: peakColour, zoomed: zoomIndex > 0 };

  const resetBest = () => {
    bestRef.current = null;
    setBest(null);
    setSharpness(null);
  };

  // Peaking switched off or on again: the overlay is cleared, or drawn by the next frame.
  const drawn = useRef(false);
  useEffect(() => {
    const canvas = overlay.current;
    if (peaking) drawn.current = true;
    else if (drawn.current && canvas) {
      canvas.getContext('2d')?.clearRect(0, 0, canvas.width, canvas.height);
    }
  }, [peaking]);
  const [colour, setColour] = useState<Colour>('off');
  const [lightError, setLightError] = useState<string | null>(null);
  const colourRef = useRef(colour);
  colourRef.current = colour;
  // Light commands go one after another; a newer one makes the older ones that wait skip.
  const generation = useRef(0);
  const queue = useRef<Promise<void>>(Promise.resolve());

  const send = (levelsToSet: Levels) => {
    const mine = ++generation.current;
    queue.current = queue.current.then(async () => {
      if (mine !== generation.current) return;
      try {
        await lightSet(levelsToSet);
      } catch (e) {
        if (mine !== generation.current) return;
        setColour('off');
        setLightError(messageOf(e));
      }
    });
  };

  const switchOff = () => {
    generation.current += 1;
    setColour('off');
    queue.current = queue.current.then(() =>
      lightOff().catch(() => {
        // Switching off is always allowed; nothing more can be done here.
      }),
    );
  };

  // Changing a slider while a colour is on sends the new levels.
  useEffect(() => {
    if (colour !== 'off' && levels) send(levelsFor(colour, levels));
    // Only a new colour or new levels send.
  }, [colour, levels]);

  // An object URL lives as long as its frame is on screen, then it is released.
  useEffect(
    () => () => {
      if (url) URL.revokeObjectURL(url);
    },
    [url],
  );

  const halt = (releaseLight = true) => {
    const current = run.current;
    if (!current) return;
    if (releaseLight && colourRef.current !== 'off') switchOff();
    current.stopped = true;
    run.current = null;
    setRunning(false);
    setUrl(null);
    setRate(null);
    // The camera goes back to the whole frame when live view is left.
    setZoomIndex(0);
    setArea(null);
    setSteps([]);
    resetBest();
    cameraPreviewStop().catch(() => {
      // Leaving live view is best effort: the backend also leaves it when a capture starts.
    });
  };
  const haltRef = useRef(halt);
  haltRef.current = halt;

  // The section switches the light off itself when it closes, after this.
  stopRef.current = () => haltRef.current(false);

  const loop = async (current: Run) => {
    const arrivals: number[] = [];
    while (!current.stopped) {
      let bytes: ArrayBuffer;
      try {
        bytes = await cameraPreviewFrame();
      } catch (e) {
        if (!current.stopped) {
          haltRef.current();
          setError(messageOf(e));
        }
        return;
      }
      if (current.stopped) return;
      const now = performance.now();
      arrivals.push(now);
      while (arrivals.length > 0 && now - (arrivals[0] ?? now) > RATE_WINDOW_MS) arrivals.shift();
      const frame = new Blob([bytes], { type: 'image/jpeg' });
      setUrl(URL.createObjectURL(frame));
      setRate(arrivals.length);
      // A frame that arrives while the last one is still measured is shown but not measured.
      if (!analysing.current && canAnalyse()) {
        analysing.current = true;
        analyser.current
          .analyse(frame, aids.current, overlay.current)
          .then((value) => {
            if (current.stopped || value === null) return;
            setSharpness(value);
            if (bestRef.current === null || value > bestRef.current) {
              bestRef.current = value;
              setBest(value);
            }
          })
          .catch(() => {
            // A frame that cannot be decoded is skipped.
          })
          .finally(() => {
            analysing.current = false;
          });
      }
    }
  };

  const start = () => {
    if (run.current) return;
    const current: Run = { stopped: false };
    run.current = current;
    setError(null);
    setLightError(null);
    setRunning(true);
    cameraPreviewZoomSteps().then(
      (found) => {
        if (!current.stopped) setSteps(found);
      },
      () => {
        // No zoom controls without the steps; the frames themselves report a real problem.
      },
    );
    void loop(current);
  };

  const chooseZoom = (index: number) => {
    const step = steps[index];
    if (!step) return;
    cameraPreviewSetZoom(step.value).then(
      () => {
        setZoomIndex(index);
        resetBest();
      },
      (e: unknown) => setError(messageOf(e)),
    );
  };

  const aim = (point: { x: number; y: number }) => {
    cameraPreviewSetArea(point.x, point.y).then(
      () => {
        setArea(point);
        resetBest();
      },
      (e: unknown) => setError(messageOf(e)),
    );
  };

  const nudge = (dx: number, dy: number) => {
    const step = nudgeStep(steps[zoomIndex]?.label ?? '');
    aim(moved(area ?? MIDDLE, dx * step, dy * step));
  };

  // A capture, test shot or calibration takes over the camera.
  // The sequence owns the light then, and lights and switches it off itself: the control only
  // goes back to Off here, without a switch-off that could cut a colour of the running shot. A
  // trip or lost connection has switched the light off already.
  useEffect(() => {
    if (blocked) haltRef.current(false);
    if ((blocked || lightLost) && colourRef.current !== 'off') {
      generation.current += 1;
      setColour('off');
    }
  }, [blocked, lightLost]);

  // Leaving the view stops the loop (the capture section also does it first, before the light).
  useEffect(() => () => haltRef.current(false), []);

  return (
    <div className={styles.captureLive}>
      <div className={styles.buttons}>
        {running ? (
          <button type="button" onClick={() => halt()}>
            {t('import.capture.liveView.stop')}
          </button>
        ) : (
          <button
            type="button"
            disabled={blocked}
            aria-describedby="import-live-view-help"
            onClick={start}
          >
            {t('import.capture.liveView.start')}
          </button>
        )}
        {running && rate !== null && (
          <p role="status" aria-live="off">
            {t('import.capture.liveView.rate', { count: rate })}
          </p>
        )}
      </div>
      <p id="import-live-view-help" className={styles.muted}>
        {t('import.capture.liveView.help')}
      </p>
      <fieldset className={styles.captureLiveLight} disabled={blocked || levels === null}>
        <legend>{t('import.capture.liveView.light.legend')}</legend>
        {COLOURS.map((choice) => (
          <label key={choice}>
            <input
              type="radio"
              name="live-view-light"
              checked={colour === choice}
              onChange={() => {
                setLightError(null);
                if (choice === 'off') switchOff();
                else setColour(choice);
              }}
            />
            {t(`import.capture.liveView.light.${choice}`)}
          </label>
        ))}
      </fieldset>
      {lightError && <p role="alert">{lightError}</p>}
      <div className={styles.captureLiveAids}>
        <label>
          <input type="checkbox" checked={peaking} onChange={(e) => setPeaking(e.target.checked)} />
          {t('import.capture.liveView.focusPeaking')}
        </label>
        {peaking && (
          <>
            <label>
              {t('import.capture.liveView.sensitivity')}
              <input
                type="range"
                min={MIN_SENSITIVITY}
                max={MAX_SENSITIVITY}
                value={sensitivity}
                onChange={(e) => setSensitivity(Number(e.target.value))}
              />
            </label>
            <label>
              {t('import.capture.liveView.peakColour')}
              <select
                value={peakColour}
                onChange={(e) => setPeakColour(e.target.value as PeakColour)}
              >
                {PEAK_CHOICES.map((choice) => (
                  <option key={choice} value={choice}>
                    {t(`import.capture.liveView.peak.${choice}`)}
                  </option>
                ))}
              </select>
            </label>
          </>
        )}
      </div>
      {running && (
        <div className={styles.captureLiveSharpness}>
          <span>
            {sharpness === null
              ? t('import.capture.liveView.sharpnessNone')
              : t('import.capture.liveView.sharpness', { value: oneDecimal(sharpness) })}
          </span>
          {best !== null && sharpness !== null && (
            <>
              <meter
                min={0}
                max={best}
                value={Math.min(sharpness, best)}
                aria-label={t('import.capture.liveView.sharpnessMeter')}
              />
              <span>{t('import.capture.liveView.best', { value: oneDecimal(best) })}</span>
            </>
          )}
          <button type="button" onClick={resetBest}>
            {t('import.capture.liveView.resetBest')}
          </button>
        </div>
      )}
      {running && steps.length > 0 && (
        <>
          <fieldset className={styles.captureLiveLight} disabled={blocked}>
            <legend>{t('import.capture.liveView.zoom')}</legend>
            {steps.map((step, index) => (
              <label key={step.value}>
                <input
                  type="radio"
                  name="live-view-zoom"
                  checked={zoomIndex === index}
                  onChange={() => chooseZoom(index)}
                />
                {index === 0 ? t('import.capture.liveView.wholeFrame') : step.label}
              </label>
            ))}
          </fieldset>
          <div
            role="group"
            aria-label={t('import.capture.liveView.move')}
            className={styles.captureLiveMove}
          >
            {ARROWS.map(({ key, dx, dy }) => (
              <button key={key} type="button" disabled={blocked} onClick={() => nudge(dx, dy)}>
                {t(`import.capture.liveView.${key}`)}
              </button>
            ))}
          </div>
          <p className={styles.muted}>{t('import.capture.liveView.areaHint')}</p>
        </>
      )}
      <div className={styles.captureLiveFrame}>
        {url ? (
          <img src={url} alt={t('import.capture.liveView.imageAlt')} draggable={false} />
        ) : (
          <span className={styles.muted}>
            {running ? t('import.capture.liveView.waiting') : t('import.capture.liveView.idle')}
          </span>
        )}
        <canvas ref={overlay} className={styles.captureLiveOverlay} aria-hidden="true" />
        {url && steps.length > 0 && zoomIndex === 0 && (
          <button
            type="button"
            className={styles.captureLivePick}
            aria-label={t('import.capture.liveView.pickArea')}
            disabled={blocked}
            onClick={(e) => {
              const box = e.currentTarget.getBoundingClientRect();
              // A key press has no position: it points at the middle.
              if (e.detail === 0 || box.width === 0 || box.height === 0) return aim(MIDDLE);
              aim({
                x: Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)),
                y: Math.min(1, Math.max(0, (e.clientY - box.top) / box.height)),
              });
            }}
          />
        )}
        {area && zoomIndex === 0 && (
          <span
            className={styles.captureLiveMarker}
            aria-hidden="true"
            style={{ left: `${area.x * 100}%`, top: `${area.y * 100}%` }}
          />
        )}
      </div>
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
