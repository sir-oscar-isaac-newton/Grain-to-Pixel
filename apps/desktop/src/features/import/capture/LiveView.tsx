import { useEffect, useRef, useState, type MutableRefObject } from 'react';
import { useTranslation } from 'react-i18next';
import { cameraPreviewFrame, cameraPreviewStop } from '../../../shared/ipc';
import { messageOf } from '../../library/library-store';
import styles from '../import.module.css';

/** The frame rate is measured over this many milliseconds. */
const RATE_WINDOW_MS = 1000;

interface LiveViewProps {
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
export function LiveView({ blocked, stopRef }: LiveViewProps) {
  const { t } = useTranslation();
  const [running, setRunning] = useState(false);
  const [url, setUrl] = useState<string | null>(null);
  const [rate, setRate] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = useRef<Run | null>(null);

  // An object URL lives as long as its frame is on screen, then it is released.
  useEffect(
    () => () => {
      if (url) URL.revokeObjectURL(url);
    },
    [url],
  );

  const halt = () => {
    const current = run.current;
    if (!current) return;
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

  stopRef.current = () => haltRef.current();

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
    setRunning(true);
    void loop(current);
  };

  // A capture, test shot or calibration takes over the camera.
  useEffect(() => {
    if (blocked) haltRef.current();
  }, [blocked]);

  // Leaving the view stops the loop (the capture section also does it first, before the light).
  useEffect(() => () => haltRef.current(), []);

  return (
    <div className={styles.captureLive}>
      <div className={styles.buttons}>
        {running ? (
          <button type="button" onClick={halt}>
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
