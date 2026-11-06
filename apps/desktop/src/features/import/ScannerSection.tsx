import { ProgressList } from '@gtp/ui';
import { useCallback, useEffect, useId, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import {
  jobCancel,
  scanPreview,
  scanPreviewCancel,
  scanResumePoint,
  scanStart,
  scannerList,
  type DeviceInfo,
  type RollPreviewImage,
} from '../../shared/ipc';
import { describeJob } from '../library/JobsPanel';
import { messageOf, useLibraryStore } from '../library/library-store';
import styles from './import.module.css';

/** The most frames a roll holds. */
export const MAX_FRAMES = 40;
/** The resolutions and sample counts the scanner is asked for. */
export const DPIS = [1000, 2000, 4000] as const;
export const SAMPLES = [1, 2, 4, 8, 16] as const;

/** A frame number as typed: a whole number from 1 to 40, or null. */
function frameOf(text: string): number | null {
  if (!/^\d+$/.test(text.trim())) return null;
  const n = Number(text);
  return n >= 1 && n <= MAX_FRAMES ? n : null;
}

/** What is wrong with the frames typed, as translation keys; nothing when they are fine. */
export function frameProblems(first: string, last: string): { first?: string; last?: string } {
  const a = frameOf(first);
  const b = frameOf(last);
  return {
    first: a === null ? 'import.scanner.frameRange' : undefined,
    last:
      b === null
        ? 'import.scanner.frameRange'
        : a !== null && b < a
          ? 'import.scanner.frameOrder'
          : undefined,
  };
}

/** A data URL for the JPEG bytes of the strip. */
function jpegUrl(bytes: number[]): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i += 8192) {
    binary += String.fromCharCode(...bytes.slice(i, i + 8192));
  }
  return `data:image/jpeg;base64,${btoa(binary)}`;
}

/**
 * Scanning a roll: the scanner, the roll and frames to scan, resolution, samples and infrared, with
 * progress and cancel from the job, resume at the next frame, and a preview of the whole roll.
 */
export function ScannerSection() {
  const { t, i18n } = useTranslation();
  const id = useId();
  const rolls = useLibraryStore((s) => s.rolls);
  const currentRoll = useLibraryStore((s) => s.rollId);
  const readOnly = useLibraryStore((s) => s.library?.read_only ?? false);
  const allJobs = useLibraryStore((s) => s.jobs);
  const addJob = useLibraryStore((s) => s.addJob);
  const dismissJob = useLibraryStore((s) => s.dismissJob);
  const jobs = allJobs.filter((j) => j.kind === 'scan');
  const scanning = jobs.some((j) => j.state === 'running');

  const [devices, setDevices] = useState<DeviceInfo[] | null>(null);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  const [chosenRoll, setChosenRoll] = useState<string | null>(null);
  const [first, setFirst] = useState('1');
  const [firstEdited, setFirstEdited] = useState(false);
  const [last, setLast] = useState(String(MAX_FRAMES));
  const [dpi, setDpi] = useState<number>(4000);
  const [samples, setSamples] = useState<number>(1);
  const [ir, setIr] = useState(true);
  const [resume, setResume] = useState(1);
  const [error, setError] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [preview, setPreview] = useState<RollPreviewImage | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);

  const rollId = chosenRoll ?? currentRoll ?? rolls[0]?.id ?? null;
  const roll = rolls.find((r) => r.id === rollId) ?? null;
  const device = devices?.[0] ?? null;
  const problems = frameProblems(first, last);
  const valid = !problems.first && !problems.last;
  const complete = resume > MAX_FRAMES;

  const refreshDevices = useCallback(() => {
    setDeviceError(null);
    scannerList().then(
      (list) => setDevices(list),
      (e: unknown) => {
        setDevices([]);
        setDeviceError(messageOf(e));
      },
    );
  }, []);

  useEffect(refreshDevices, [refreshDevices]);

  /** Asks where this roll's scanning should continue, now and whenever a scan ends. */
  useEffect(() => {
    if (!rollId || scanning) return;
    let alive = true;
    scanResumePoint(rollId).then(
      (n) => {
        if (!alive) return;
        setResume(n);
        if (!firstEdited) setFirst(String(Math.min(n, MAX_FRAMES)));
      },
      () => {
        // The roll was just removed; the picker follows the store.
      },
    );
    return () => {
      alive = false;
    };
  }, [rollId, scanning, firstEdited]);

  const startFrom = (n: number) => {
    setFirstEdited(true);
    setFirst(String(n));
  };

  const start = async () => {
    if (!roll || !device || !valid) return;
    setError(null);
    try {
      const job = await scanStart(roll.id, {
        device_id: device.id,
        first_frame: Number(first),
        last_frame: Number(last),
        dpi,
        samples,
        ir,
        autofocus: false,
      });
      addJob({ id: job, kind: 'scan', rollName: roll.name, fraction: 0, state: 'running' });
    } catch (e) {
      setError(t('import.scanner.startFailed', { message: messageOf(e) }));
    }
  };

  const previewRoll = async () => {
    if (!device) return;
    setPreviewing(true);
    setPreview(null);
    setPreviewError(null);
    try {
      setPreview(await scanPreview(device.id));
    } catch (e) {
      // A cancel is the user's own doing, not a failure.
      const cancelled = (e as { error?: { code?: string } }).error?.code === 'Cancelled';
      if (!cancelled) setPreviewError(t('import.scanner.previewFailed', { message: messageOf(e) }));
    } finally {
      setPreviewing(false);
    }
  };

  const locked = scanning || readOnly;

  return (
    <section aria-labelledby={`${id}-heading`} className={styles.section}>
      <h3 id={`${id}-heading`} className={styles.heading}>
        {t('import.scanner.heading')}
      </h3>
      <p className={styles.muted}>{t('import.scanner.intro')}</p>

      {devices === null ? (
        <p role="status">{t('import.scanner.looking')}</p>
      ) : device ? (
        <p>
          {t('import.scanner.device', { name: device.name })}{' '}
          <button type="button" disabled={scanning} onClick={refreshDevices}>
            {t('import.scanner.refresh')}
          </button>
        </p>
      ) : (
        <div>
          <p role="alert">{deviceError ?? t('import.scanner.none')}</p>
          <p className={styles.muted}>{t('import.scanner.driverHint')}</p>
          <button type="button" onClick={refreshDevices}>
            {t('import.scanner.refresh')}
          </button>
        </div>
      )}

      {device && rolls.length > 0 && (
        <>
          <div className={styles.row}>
            <label className={styles.field}>
              {t('import.files.roll')}
              <select
                value={rollId ?? ''}
                disabled={locked}
                onChange={(e) => {
                  setChosenRoll(e.target.value);
                  setFirstEdited(false);
                }}
              >
                {rolls.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.name}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <p className={styles.muted}>
            {complete
              ? t('import.scanner.complete')
              : resume > 1
                ? t('import.scanner.resume', { n: resume })
                : t('import.scanner.fresh')}{' '}
            {resume > 1 && (
              <button type="button" disabled={locked} onClick={() => startFrom(1)}>
                {t('import.scanner.fromStart')}
              </button>
            )}
          </p>

          <div className={styles.row}>
            <label className={styles.field}>
              {t('import.scanner.first')}
              <input
                type="number"
                inputMode="numeric"
                min={1}
                max={MAX_FRAMES}
                value={first}
                disabled={locked}
                aria-invalid={problems.first ? true : undefined}
                aria-describedby={problems.first ? `${id}-first-problem` : undefined}
                onChange={(e) => {
                  setFirstEdited(true);
                  setFirst(e.target.value);
                }}
              />
            </label>
            <label className={styles.field}>
              {t('import.scanner.last')}
              <input
                type="number"
                inputMode="numeric"
                min={1}
                max={MAX_FRAMES}
                value={last}
                disabled={locked}
                aria-invalid={problems.last ? true : undefined}
                aria-describedby={problems.last ? `${id}-last-problem` : undefined}
                onChange={(e) => setLast(e.target.value)}
              />
            </label>
            <label className={styles.field}>
              {t('import.scanner.dpi')}
              <select
                value={dpi}
                disabled={locked}
                onChange={(e) => setDpi(Number(e.target.value))}
              >
                {DPIS.map((d) => (
                  <option key={d} value={d}>
                    {t('import.scanner.dpiOption', { dpi: d })}
                  </option>
                ))}
              </select>
            </label>
            <label className={styles.field}>
              {t('import.scanner.samples')}
              <select
                value={samples}
                disabled={locked}
                onChange={(e) => setSamples(Number(e.target.value))}
              >
                {SAMPLES.map((s) => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <input
                type="checkbox"
                checked={ir}
                disabled={locked}
                onChange={(e) => setIr(e.target.checked)}
              />{' '}
              {t('import.scanner.ir')}
            </label>
          </div>
          {problems.first && (
            <p id={`${id}-first-problem`} className={styles.problem}>
              {t(problems.first, { max: MAX_FRAMES })}
            </p>
          )}
          {problems.last && (
            <p id={`${id}-last-problem`} className={styles.problem}>
              {t(problems.last, { max: MAX_FRAMES })}
            </p>
          )}

          <div className={styles.buttons}>
            <button
              type="button"
              disabled={locked || !valid || !roll || complete}
              onClick={() => void start()}
            >
              {valid
                ? t('import.scanner.start', {
                    count: Number(last) - Number(first) + 1,
                    first,
                    last,
                  })
                : t('import.scanner.startPlain')}
            </button>
            <button
              type="button"
              disabled={locked || previewing}
              onClick={() => void previewRoll()}
            >
              {previewing ? t('import.scanner.previewing') : t('import.scanner.preview')}
            </button>
            {previewing && (
              <button type="button" onClick={() => void scanPreviewCancel()}>
                {t('import.scanner.previewCancel')}
              </button>
            )}
          </div>
        </>
      )}

      {error && <p role="alert">{error}</p>}
      {previewError && <p role="alert">{previewError}</p>}

      {preview && (
        <figure className={styles.preview}>
          <div className={styles.previewScroll}>
            <div
              className={styles.previewStrip}
              style={{ '--pitch': `${preview.pitch}px` } as CSSProperties}
            >
              <img
                src={jpegUrl(preview.strip_jpeg)}
                width={preview.strip_width}
                height={preview.strip_height}
                alt={t('import.scanner.previewAlt', { count: preview.frame_count })}
              />
            </div>
          </div>
          <figcaption className={styles.muted}>
            {t('import.scanner.previewCaption', { count: preview.frame_count })}
          </figcaption>
        </figure>
      )}

      <ProgressList
        items={jobs.map((job) => describeJob(job, t, i18n.language))}
        label={t('jobs.label')}
        cancelLabel={t('jobs.cancel')}
        dismissLabel={t('jobs.dismiss')}
        onCancel={(jobId) => void jobCancel(jobId)}
        onDismiss={dismissJob}
      />
    </section>
  );
}
