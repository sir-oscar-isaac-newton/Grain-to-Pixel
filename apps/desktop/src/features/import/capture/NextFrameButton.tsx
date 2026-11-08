import { StatusBadge } from '@gtp/ui';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import {
  captureTriplet,
  frameList,
  jobCancel,
  onCaptureStep,
  type ImportWarning,
  type CaptureStep,
  type Levels,
  type Roll,
} from '../../../shared/ipc';
import { messageOf, useLibraryStore } from '../../library/library-store';
import { useThumbnail } from '../../library/use-thumbnail';
import styles from '../import.module.css';

const CHANNELS = ['red', 'green', 'blue'] as const;

type Steps = Partial<Record<CaptureStep['channel'], CaptureStep['state']>>;

interface Result {
  frameId: string | null;
  warnings: ImportWarning[];
  /** How many frames the roll holds now; null while that is being asked. */
  count: number | null;
}

/** What a capture warned about, in plain words, once for each kind (and each clipped colour). */
export function warningTexts(warnings: ImportWarning[], t: TFunction): string[] {
  const texts = new Map<string, string>();
  for (const { kind } of warnings) {
    switch (kind.kind) {
      case 'high_temperature':
        texts.set(kind.kind, t('import.capture.warn.temperature', { temp: kind.temp_c }));
        break;
      case 'low_supply':
        texts.set(kind.kind, t('import.capture.warn.supply', { mv: kind.vbus_mv }));
        break;
      case 'exposure_time_mismatch':
      case 'iso_mismatch':
      case 'aperture_mismatch':
        texts.set(kind.kind, t(`import.capture.warn.${kind.kind}`));
        break;
      case 'clipped':
        texts.set(
          `clipped-${kind.channel}`,
          t('import.capture.warn.clipped', {
            channel: t(`import.channel.${kind.channel}`),
            pct: (kind.per_mille / 10).toFixed(1),
          }),
        );
        break;
      case 'flat_field_skipped':
        texts.set(kind.kind, t('import.capture.warn.flat_field_skipped'));
        break;
      case 'exposure_differs_from_levels':
        texts.set(kind.kind, t('import.capture.warn.exposure_differs_from_levels'));
        break;
      default:
        break;
    }
  }
  return [...texts.values()];
}

/** True when a key press on this element belongs to the element, so Space must not capture. */
function ownsSpace(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target.closest(
      'button, input, select, textarea, a[href], summary, [role="slider"], [role="button"], [role="dialog"], [contenteditable="true"]',
    ) !== null
  );
}

function ResultThumbnail({ id }: { id: string }) {
  const { t } = useTranslation();
  const thumbnail = useThumbnail(id, true);
  if (thumbnail.status === 'ready') {
    return (
      <img
        src={thumbnail.url}
        alt={t('import.capture.next.thumbnail')}
        draggable={false}
        className={styles.captureThumb}
      />
    );
  }
  if (thumbnail.status === 'loading') return <span aria-hidden="true" />;
  return <StatusBadge tone="warn">{t('import.capture.next.noPreview')}</StatusBadge>;
}

interface NextFrameButtonProps {
  /** The light and the camera are there and usable. */
  ready: boolean;
  /** The roll the frame goes into (chosen in the section), or null when there is none. */
  roll: Roll | null;
  /** The working levels; null until they are known. */
  levels: Levels | null;
  /** Called with true while a capture starts or runs, and false when it is over. */
  onBusyChange: (busy: boolean) => void;
}

/**
 * The large button that captures the next frame (also the Space key): red, green and blue
 * exposures merged and stored in the chosen roll, with the steps while it runs, the result after,
 * and what the capture warned about.
 */
export function NextFrameButton({ ready, roll, levels, onBusyChange }: NextFrameButtonProps) {
  const { t } = useTranslation();
  const readOnly = useLibraryStore((s) => s.library?.read_only ?? false);
  const addJob = useLibraryStore((s) => s.addJob);

  const [jobId, setJobId] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [steps, setSteps] = useState<Steps>({});
  const [result, setResult] = useState<Result | null>(null);
  const [error, setError] = useState<string | null>(null);

  const job = useLibraryStore((s) => s.jobs.find((j) => j.id === jobId));
  const jobState = job?.state;
  const running = starting || jobState === 'running';

  const onBusyChangeRef = useRef(onBusyChange);
  onBusyChangeRef.current = onBusyChange;
  useEffect(() => {
    onBusyChangeRef.current(running);
  }, [running]);

  const canStart = ready && !running && !readOnly && roll !== null && levels !== null;

  // The steps of the running capture.
  useEffect(() => {
    let alive = true;
    let stop: (() => void) | undefined;
    onCaptureStep((step) => setSteps((all) => ({ ...all, [step.channel]: step.state }))).then(
      (stopListening) => {
        if (alive) stop = stopListening;
        else stopListening();
      },
      () => {
        // No backend (tests, plain browser): there are no steps to follow.
      },
    );
    return () => {
      alive = false;
      stop?.();
    };
  }, []);

  // What came of the capture: the new frame, its warnings and the roll's frame count.
  const frameId = job?.importedFrames?.[0] ?? null;
  const warnings = job?.warnings;
  const rollOfJob = useRef<string | null>(null);
  useEffect(() => {
    if (jobState === 'error') {
      setError(job?.message ?? t('import.capture.next.failed'));
      return;
    }
    if (jobState !== 'done') return;
    const forRoll = rollOfJob.current;
    setResult({ frameId, warnings: warnings ?? [], count: null });
    if (forRoll === null) return;
    let alive = true;
    frameList({ roll_id: forRoll, page: 0, page_size: 1 }).then(
      (page) => {
        if (alive) setResult((r) => (r ? { ...r, count: page.total } : r));
      },
      () => {
        // The counter stays hidden; the frame itself was stored.
      },
    );
    return () => {
      alive = false;
    };
    // The job's own fields are read once, when it finishes.
  }, [jobState]);

  const start = async () => {
    if (!roll || !levels) return;
    setStarting(true);
    setError(null);
    setResult(null);
    setSteps({});
    rollOfJob.current = roll.id;
    try {
      const id = await captureTriplet(roll.id, levels);
      addJob({ id, kind: 'capture', rollName: roll.name, fraction: 0, state: 'running' });
      setJobId(id);
    } catch (e) {
      setError(messageOf(e));
    } finally {
      setStarting(false);
    }
  };

  // Space is fixed (the key map refuses it as a binding), so it is handled here.
  const startRef = useRef(start);
  startRef.current = start;
  useEffect(() => {
    if (!canStart) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== ' ' || event.repeat || event.defaultPrevented) return;
      if (event.ctrlKey || event.metaKey || event.altKey || ownsSpace(event.target)) return;
      event.preventDefault();
      void startRef.current();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [canStart]);

  const texts = result ? warningTexts(result.warnings, t) : [];

  return (
    <div className={styles.captureNextBox}>
      <div className={styles.buttons}>
        <button
          type="button"
          className={styles.captureNext}
          disabled={!canStart}
          onClick={() => void start()}
        >
          {running ? t('import.capture.next.running') : t('import.capture.next.label')}
        </button>
        {jobState === 'running' && jobId && (
          <button type="button" onClick={() => void jobCancel(jobId)}>
            {t('import.capture.next.cancel')}
          </button>
        )}
      </div>
      {readOnly && <p role="status">{t('import.capture.next.readOnly')}</p>}

      {running && (
        <ol aria-label={t('import.capture.next.steps')} className={styles.captureSteps}>
          {CHANNELS.map((channel) => (
            <li key={channel}>
              {t('import.capture.next.step', {
                channel: t(`import.channel.${channel}`),
                state: t(`import.capture.next.state.${steps[channel] ?? 'waiting'}`),
              })}
            </li>
          ))}
        </ol>
      )}

      {error && <p role="alert">{error}</p>}
      {jobState === 'cancelled' && <p role="status">{t('import.capture.next.cancelled')}</p>}

      {result && (
        <div className={styles.captureResult}>
          {result.frameId && <ResultThumbnail id={result.frameId} />}
          <div>
            <p role="status">
              {result.count === null
                ? t('import.capture.next.stored')
                : t('import.capture.next.counter', { count: result.count })}
            </p>
            {texts.map((text) => (
              <p key={text} role="alert">
                {text}
              </p>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
