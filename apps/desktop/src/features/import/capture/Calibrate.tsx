import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  captureCalibrate,
  jobCancel,
  type ExposureInfo,
  type Levels,
  type Roll,
} from '../../../shared/ipc';
import { messageOf, useLibraryStore } from '../../library/library-store';
import styles from '../import.module.css';

const CHANNELS = ['red', 'green', 'blue'] as const;

interface CalibrateProps {
  /** The light and the camera are there and usable, and no camera setting is changing. */
  ready: boolean;
  /** True while any capture or calibration runs (this one included). */
  busy: boolean;
  /** The roll the calibration belongs to (chosen in the section), or null when there is none. */
  roll: Roll | null;
  /** The working levels it starts from; null until they are known. */
  levels: Levels | null;
  /**
   * Called once, when the calibration finishes, with the levels found and the camera settings
   * its shots were taken with (they count as the test shot the save needs).
   */
  onCalibrated: (levels: Levels, exposure: ExposureInfo) => void;
  /** Called with true while a calibration starts or runs, and false when it is over. */
  onBusyChange: (busy: boolean) => void;
}

/**
 * Finds the Scanlight level of each colour by itself, on a frame of clear film base. Shows what
 * it is doing, then what it found; the levels go to the sliders and nothing is saved.
 */
export function Calibrate({
  ready,
  busy,
  roll,
  levels,
  onCalibrated,
  onBusyChange,
}: CalibrateProps) {
  const { t } = useTranslation();
  const addJob = useLibraryStore((s) => s.addJob);
  const [jobId, setJobId] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const job = useLibraryStore((s) => s.jobs.find((j) => j.id === jobId));
  const jobState = job?.state;
  const running = starting || jobState === 'running';
  const onBusyChangeRef = useRef(onBusyChange);
  onBusyChangeRef.current = onBusyChange;
  useEffect(() => {
    onBusyChangeRef.current(running);
  }, [running]);
  const found = jobState === 'done' ? (job?.calibrated ?? null) : null;

  const onCalibratedRef = useRef(onCalibrated);
  onCalibratedRef.current = onCalibrated;
  useEffect(() => {
    if (found) {
      const { exposure_time, iso, f_number } = found.exposure;
      onCalibratedRef.current(found.levels, {
        exposure_time: exposure_time ?? null,
        iso: iso ?? null,
        f_number: f_number ?? null,
      });
    }
    // The result is taken once, when the job finishes.
  }, [found]);

  const start = async () => {
    if (!roll || !levels) return;
    setStarting(true);
    setError(null);
    setJobId(null);
    try {
      const id = await captureCalibrate(roll.id, levels);
      addJob({ id, kind: 'calibration', rollName: roll.name, fraction: 0, state: 'running' });
      setJobId(id);
    } catch (e) {
      setError(messageOf(e));
    } finally {
      setStarting(false);
    }
  };

  const failure =
    jobState === 'error' ? (job?.message ?? t('import.capture.calibrate.failed')) : null;
  const problem = error ?? failure;

  return (
    <div className={styles.captureTest}>
      <div className={styles.buttons}>
        <button
          type="button"
          disabled={!ready || busy || running || roll === null || levels === null}
          aria-describedby="import-calibrate-help"
          onClick={() => void start()}
        >
          {running ? t('import.capture.calibrate.running') : t('import.capture.calibrate.label')}
        </button>
        {jobState === 'running' && jobId && (
          <button type="button" onClick={() => void jobCancel(jobId)}>
            {t('import.capture.calibrate.cancel')}
          </button>
        )}
      </div>
      <p id="import-calibrate-help" className={styles.muted}>
        {t('import.capture.calibrate.help')}
      </p>
      {jobState === 'running' && (
        <p role="status">{job?.stage || t('import.capture.calibrate.starting')}</p>
      )}
      {problem && <p role="alert">{problem}</p>}
      {jobState === 'cancelled' && <p role="status">{t('import.capture.calibrate.cancelled')}</p>}
      {found && (
        <div>
          <ul className={styles.captureCalibration}>
            {CHANNELS.map((channel, i) => {
              const reached = found.reached[i] ?? null;
              return (
                <li key={channel}>
                  {t('import.capture.calibrate.result', {
                    channel: t(`import.capture.calibrate.channel.${channel}`),
                    level: found.levels[i],
                    reached: reached === null ? '–' : `${Math.round(reached * 100)} %`,
                    count: found.shots[i],
                  })}
                </li>
              );
            })}
          </ul>
          {found.warnings.map((warning) => (
            <p key={warning.channel} role="alert">
              {t('import.capture.calibrate.coarse', {
                channel: t(`import.capture.calibrate.channel.${warning.channel}`),
                level: warning.level,
              })}
            </p>
          ))}
        </div>
      )}
    </div>
  );
}
