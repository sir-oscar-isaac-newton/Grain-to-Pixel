import { ProgressList } from '@gtp/ui';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { formatBytes } from '../../shared/format';
import { jobCancel, libraryVerify, type VerifyResult } from '../../shared/ipc';
import { describeJob } from '../library/JobsPanel';
import { messageOf, useLibraryStore } from '../library/library-store';
import styles from './settings.module.css';

interface VerifySectionProps {
  /** A check ended (the library then has a new "last verified"), so the figures can be read again. */
  onFinished: () => void;
}

/** What a finished check found, in words. */
function Findings({ result }: { result: VerifyResult }) {
  const { t, i18n } = useTranslation();
  const { problems, thumbnails_missing: thumbnails, stray_folders: strays } = result;
  const nothing = problems.length === 0 && thumbnails === 0 && strays.length === 0;
  return (
    <div role="status" className={styles.result}>
      <p>
        {result.bytes_checked === null
          ? t('settings.verify.checked', { count: result.files_checked })
          : t('settings.verify.checkedSize', {
              count: result.files_checked,
              size: formatBytes(result.bytes_checked, i18n.language),
            })}
      </p>
      {nothing && <p>{t('settings.verify.clean')}</p>}
      {problems.length === 0 && !nothing && <p>{t('settings.verify.originalsFine')}</p>}
      {problems.length > 0 && (
        <>
          <h4 className={styles.subheading}>
            {t('settings.verify.problemsHeading', { count: problems.length })}
          </h4>
          <ul className={styles.problems}>
            {problems.map((problem) => (
              <li key={`${problem.frame_id}-${problem.role}-${problem.rel_path}`}>
                {t('settings.verify.problem', {
                  path: problem.rel_path,
                  reason: t(`settings.verify.reason.${problem.reason}`),
                })}
              </li>
            ))}
          </ul>
        </>
      )}
      {thumbnails > 0 && <p>{t('settings.verify.thumbnails', { count: thumbnails })}</p>}
      {strays.length > 0 && (
        <>
          <h4 className={styles.subheading}>{t('settings.verify.strayHeading')}</h4>
          <ul className={styles.problems}>
            {strays.map((folder) => (
              <li key={folder}>{folder}</li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

/**
 * Checks the library's originals (the verify job): a button, the progress with Cancel, and what
 * the check found. The job only reads; the state lives in the library store, so leaving Settings
 * and coming back shows the same check.
 */
export function VerifySection({ onFinished }: VerifySectionProps) {
  const { t, i18n } = useTranslation();
  const job = useLibraryStore((s) => s.jobs.find((j) => j.kind === 'verify'));
  const addJob = useLibraryStore((s) => s.addJob);
  const dismissJob = useLibraryStore((s) => s.dismissJob);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const running = job?.state === 'running';
  const wasRunning = useRef(running);
  useEffect(() => {
    if (wasRunning.current && !running) onFinished();
    wasRunning.current = running;
  }, [running, onFinished]);

  const start = async () => {
    if (running || starting) return;
    setStarting(true);
    setError(null);
    try {
      const id = await libraryVerify();
      // Only the newest check is kept.
      useLibraryStore
        .getState()
        .jobs.filter((j) => j.kind === 'verify')
        .forEach((j) => dismissJob(j.id));
      addJob({ id, kind: 'verify', rollName: '', fraction: 0, state: 'running' });
    } catch (e) {
      setError(t('settings.verify.startFailed', { message: messageOf(e) }));
    } finally {
      setStarting(false);
    }
  };

  return (
    <section aria-labelledby="settings-verify" className={styles.section}>
      <h3 id="settings-verify" className={styles.heading}>
        {t('settings.verify.heading')}
      </h3>
      <p className={styles.muted}>{t('settings.verify.intro')}</p>
      <div className={styles.buttons}>
        <button type="button" disabled={running || starting} onClick={() => void start()}>
          {t('settings.verify.start')}
        </button>
      </div>
      {error && <p role="alert">{error}</p>}
      {job && (
        <ProgressList
          items={[describeJob(job, t, i18n.language)]}
          label={t('settings.verify.running')}
          cancelLabel={t('jobs.cancel')}
          dismissLabel={t('jobs.dismiss')}
          onCancel={(id) => void jobCancel(id)}
          onDismiss={dismissJob}
        />
      )}
      {job?.state === 'cancelled' && (
        <p className={styles.muted}>{t('settings.verify.cancelled')}</p>
      )}
      {job?.state === 'done' && job.verified && <Findings result={job.verified} />}
    </section>
  );
}
