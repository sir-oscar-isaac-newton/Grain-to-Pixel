import { ProgressList, type ProgressItem } from '@gtp/ui';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import { jobCancel } from '../../shared/ipc';
import styles from './library.module.css';
import { useLibraryStore, type JobRow } from './library-store';

/** A job as the progress list shows it: what it is, how far it got, and what came of it. */
export function describeJob(job: JobRow, t: TFunction, language: string): ProgressItem {
  const parts: string[] = [];
  if (job.state === 'done' && job.imported !== undefined) {
    parts.push(t('jobs.imported', { count: job.imported }));
  }
  if (job.skipped && job.skipped.length > 0) {
    parts.push(t('jobs.skipped', { count: job.skipped.length }));
  }
  if (job.state === 'done' && job.exported !== undefined) {
    parts.push(t('jobs.exported', { count: job.exported.length }));
  }
  if (job.failed && job.failed.length > 0) {
    parts.push(t('jobs.exportFailed', { count: job.failed.length }));
  }
  if (job.state === 'cancelled' && job.kind === 'export') {
    parts.push(t('jobs.exportCancelled'));
  }
  if (job.message) parts.push(job.message);
  return {
    id: job.id,
    label:
      job.kind === 'export'
        ? t('jobs.export', { count: job.frames ?? 0 })
        : t('jobs.import', { roll: job.rollName }),
    fraction: job.fraction,
    state: job.state,
    stateLabel: t(`jobs.${job.state}`),
    detail:
      parts.length > 0
        ? new Intl.ListFormat(language, { style: 'narrow', type: 'conjunction' }).format(parts)
        : undefined,
    extra:
      job.skipped && job.skipped.length > 0 ? (
        <details className={styles.skipped}>
          <summary>{t('jobs.skippedFiles')}</summary>
          <ul>
            {job.skipped.map((file) => (
              <li key={`${file.path}-${file.reason}`}>
                {t('jobs.skippedFile', {
                  path: file.path,
                  reason: t(`jobs.skip.${file.reason}`),
                })}
              </li>
            ))}
          </ul>
        </details>
      ) : job.failed && job.failed.length > 0 ? (
        <details className={styles.skipped}>
          <summary>{t('jobs.failedFrames')}</summary>
          <ul>
            {job.failed.map((failure) => (
              <li key={failure.frame_id}>{failure.message}</li>
            ))}
          </ul>
        </details>
      ) : undefined,
  };
}

/** The jobs started from this screen, with progress, cancel and the result of each. */
export function JobsPanel() {
  const { t, i18n } = useTranslation();
  const jobs = useLibraryStore((s) => s.jobs);
  const dismissJob = useLibraryStore((s) => s.dismissJob);

  return (
    <ProgressList
      items={jobs.map((job) => describeJob(job, t, i18n.language))}
      label={t('jobs.label')}
      cancelLabel={t('jobs.cancel')}
      dismissLabel={t('jobs.dismiss')}
      onCancel={(id) => void jobCancel(id)}
      onDismiss={dismissJob}
    />
  );
}
