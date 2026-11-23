import * as AlertDialog from '@radix-ui/react-alert-dialog';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { captureFlatField, jobCancel, type Levels, type Roll } from '../../../shared/ipc';
import libraryStyles from '../../library/library.module.css';
import { messageOf, useLibraryStore } from '../../library/library-store';
import styles from '../import.module.css';
import { CaptureSteps, useCaptureSteps } from './CaptureSteps';
import { formatAperture } from './exposure';

/** What the job says while it shoots the three pictures and stores them (the backend's words). */
const CAPTURE_STAGE = 'Capturing the flat field';

interface FlatFieldCaptureProps {
  /** The light and the camera are there and usable, and nothing else is running. */
  ready: boolean;
  /** The roll the flat field goes into, or null when there is none. */
  roll: Roll | null;
  /** The working levels; null until they are known. */
  levels: Levels | null;
  /** Called with true while the capture starts or runs, and false when it is over. */
  onBusyChange: (busy: boolean) => void;
}

/**
 * Captures the roll's flat field: asks first (the film must be out of the carrier, and a flat field
 * the roll has is replaced), then shoots the bare light as a frame is shot and attaches it.
 */
export function FlatFieldCapture({ ready, roll, levels, onBusyChange }: FlatFieldCaptureProps) {
  const { t } = useTranslation();
  const readOnly = useLibraryStore((s) => s.library?.read_only ?? false);
  const addJob = useLibraryStore((s) => s.addJob);
  const reloadRolls = useLibraryStore((s) => s.reloadRolls);

  const [asking, setAsking] = useState(false);
  const [jobId, setJobId] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [steps, resetSteps] = useCaptureSteps();
  const [error, setError] = useState<string | null>(null);

  const job = useLibraryStore((s) => s.jobs.find((j) => j.id === jobId));
  const jobState = job?.state;
  const running = starting || jobState === 'running';
  // The job calibrates the light first (its calibration shots also send steps, which say nothing
  // about the flat field), then shoots the three pictures: the steps show for those only.
  const capturing = job?.stage === CAPTURE_STAGE;

  const onBusyChangeRef = useRef(onBusyChange);
  onBusyChangeRef.current = onBusyChange;
  useEffect(() => {
    onBusyChangeRef.current(running);
  }, [running]);

  useEffect(() => {
    // Forget what the calibration's shots reported when the three pictures start.
    if (capturing) resetSteps();
  }, [capturing]);

  useEffect(() => {
    if (jobState === 'error') {
      setError(job?.message ?? t('import.capture.flatField.failed'));
    } else if (jobState === 'done') {
      // The roll now has the flat field: the library's status follows.
      reloadRolls().catch(() => {
        // The next change or a restart reloads the rolls.
      });
    }
    // The job's own fields are read once, when it finishes.
  }, [jobState]);

  const canStart = ready && !running && !readOnly && roll !== null && levels !== null;

  const start = async () => {
    if (!roll || !levels) return;
    setAsking(false);
    setStarting(true);
    setError(null);
    setJobId(null);
    resetSteps();
    try {
      const id = await captureFlatField(roll.id, levels);
      addJob({ id, kind: 'flat_field', rollName: roll.name, fraction: 0, state: 'running' });
      setJobId(id);
    } catch (e) {
      setError(messageOf(e));
    } finally {
      setStarting(false);
    }
  };

  const aperture = formatAperture(job?.flatField?.exposure.f_number);
  const usedLevels = job?.flatFieldLevels?.join(', ');
  const doneText = usedLevels
    ? aperture
      ? t('import.capture.flatField.doneApertureLevels', { aperture, levels: usedLevels })
      : t('import.capture.flatField.doneLevels', { levels: usedLevels })
    : aperture
      ? t('import.capture.flatField.doneAperture', { aperture })
      : t('import.capture.flatField.done');

  return (
    <div className={styles.captureNextBox}>
      <div className={styles.buttons}>
        <button type="button" disabled={!canStart} onClick={() => setAsking(true)}>
          {t('import.capture.flatField.label')}
        </button>
        {jobState === 'running' && jobId && (
          <button type="button" onClick={() => void jobCancel(jobId)}>
            {t('import.capture.flatField.stop')}
          </button>
        )}
      </div>
      {readOnly && <p role="status">{t('import.capture.flatField.readOnly')}</p>}

      {running && job?.stage && <p role="status">{job.stage}</p>}
      {running && capturing && <CaptureSteps steps={steps} />}

      {error && <p role="alert">{error}</p>}
      {jobState === 'cancelled' && <p role="status">{t('import.capture.flatField.cancelled')}</p>}
      {jobState === 'done' && <p role="status">{doneText}</p>}

      <AlertDialog.Root open={asking} onOpenChange={setAsking}>
        <AlertDialog.Portal>
          <AlertDialog.Overlay className={libraryStyles.overlay} />
          <AlertDialog.Content className={libraryStyles.dialog}>
            <AlertDialog.Title>{t('import.capture.flatField.title')}</AlertDialog.Title>
            <AlertDialog.Description>{t('import.capture.flatField.body')}</AlertDialog.Description>
            {roll?.has_flat_field && <p>{t('import.capture.flatField.replace')}</p>}
            <div className={libraryStyles.buttons}>
              <AlertDialog.Cancel asChild>
                <button type="button">{t('import.capture.flatField.cancel')}</button>
              </AlertDialog.Cancel>
              <button type="button" onClick={() => void start()}>
                {t('import.capture.flatField.confirm')}
              </button>
            </div>
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>
    </div>
  );
}
