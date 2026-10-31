import { ProgressList, type ProgressItem } from '@gtp/ui';
import { useTranslation } from 'react-i18next';
import { jobCancel } from '../../shared/ipc';
import styles from './library.module.css';
import { useLibraryStore, type JobRow } from './library-store';

/** The jobs started from this screen, with progress, cancel and the result of each. */
export function JobsPanel() {
  const { t, i18n } = useTranslation();
  const jobs = useLibraryStore((s) => s.jobs);
  const dismissJob = useLibraryStore((s) => s.dismissJob);

  const describe = (job: JobRow): ProgressItem => {
    const parts: string[] = [];
    if (job.state === 'done' && job.imported !== undefined) {
      parts.push(t('jobs.imported', { count: job.imported }));
    }
    if (job.skipped && job.skipped.length > 0) {
      parts.push(t('jobs.skipped', { count: job.skipped.length }));
    }
    if (job.message) parts.push(job.message);
    return {
      id: job.id,
      label: t('jobs.import', { roll: job.rollName }),
      fraction: job.fraction,
      state: job.state,
      stateLabel: t(`jobs.${job.state}`),
      detail:
        parts.length > 0
          ? new Intl.ListFormat(i18n.language, { style: 'narrow', type: 'conjunction' }).format(
              parts,
            )
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
        ) : undefined,
    };
  };

  return (
    <ProgressList
      items={jobs.map(describe)}
      label={t('jobs.label')}
      cancelLabel={t('jobs.cancel')}
      dismissLabel={t('jobs.dismiss')}
      onCancel={(id) => void jobCancel(id)}
      onDismiss={dismissJob}
    />
  );
}
