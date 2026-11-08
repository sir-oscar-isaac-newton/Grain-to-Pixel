import { useEffect, useRef, useState, type MutableRefObject } from 'react';
import { useTranslation } from 'react-i18next';
import {
  cameraPreviewFrame,
  cameraPreviewStop,
  lightOff,
  lightSet,
  type Levels,
} from '../../../shared/ipc';
import { messageOf } from '../../library/library-store';
import styles from '../import.module.css';

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
      setUrl(URL.createObjectURL(new Blob([bytes], { type: 'image/jpeg' })));
      setRate(arrivals.length);
    }
  };

  const start = () => {
    if (run.current) return;
    const current: Run = { stopped: false };
    run.current = current;
    setError(null);
    setLightError(null);
    setRunning(true);
    void loop(current);
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
      <div className={styles.captureLiveFrame}>
        {url ? (
          <img src={url} alt={t('import.capture.liveView.imageAlt')} draggable={false} />
        ) : (
          <span className={styles.muted}>
            {running ? t('import.capture.liveView.waiting') : t('import.capture.liveView.idle')}
          </span>
        )}
      </div>
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
