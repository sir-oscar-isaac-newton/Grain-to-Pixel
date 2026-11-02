import { ProgressList, type ProgressItem } from '@gtp/ui';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import { jobCancel, type ImportWarning } from '../../shared/ipc';
import styles from './library.module.css';
import { useLibraryStore, type JobRow } from './library-store';

/** A frame number for a warning: where the frame came in the import, counted from 1. */
function frameNumber(job: JobRow, frameId: string): number {
  return (job.importedFrames?.indexOf(frameId) ?? -1) + 1;
}

/** One warning of a triplet import, in words. */
function warningText(job: JobRow, warning: ImportWarning, t: TFunction): string {
  const frame = frameNumber(job, warning.frame_id);
  const kind = warning.kind;
  const what =
    kind.kind === 'clipped'
      ? t('jobs.warning.clipped', {
          channel: t(`import.channel.${kind.channel}`),
          percent: (kind.per_mille / 10).toLocaleString(undefined, { maximumFractionDigits: 1 }),
        })
      : t(`jobs.warning.${kind.kind}`);
  return t('jobs.warningLine', { frame, what });
}

/** What came of a job beyond one line: the skipped files, the failed frames, the warnings. */
function JobDetails({ job, t }: { job: JobRow; t: TFunction }) {
  const skipped = job.skipped ?? [];
  const failed = job.failed ?? [];
  const warnings = job.warnings ?? [];
  const base = job.baseIsGuess === true || job.baseReplaced === true;
  if (skipped.length === 0 && failed.length === 0 && warnings.length === 0 && !base) {
    return undefined;
  }
  return (
    <>
      {job.baseIsGuess === true && (
        <p className={styles.hint} role="note">
          {t('jobs.baseGuess')}
        </p>
      )}
      {job.baseReplaced === true && (
        <p className={styles.hint} role="note">
          {t('jobs.baseReplaced')}
        </p>
      )}
      {skipped.length > 0 && (
        <details className={styles.skipped}>
          <summary>{t('jobs.skippedFiles')}</summary>
          <ul>
            {skipped.map((file) => (
              <li key={`${file.path}-${file.reason}`}>
                {t('jobs.skippedFile', {
                  path: file.path,
                  reason: t(`jobs.skip.${file.reason}`),
                })}
              </li>
            ))}
          </ul>
        </details>
      )}
      {failed.length > 0 && (
        <details className={styles.skipped}>
          <summary>{t('jobs.failedFrames')}</summary>
          <ul>
            {failed.map((failure) => (
              <li key={failure.frame_id}>{failure.message}</li>
            ))}
          </ul>
        </details>
      )}
      {warnings.length > 0 && (
        <details className={styles.skipped}>
          <summary>{t('jobs.warningsList')}</summary>
          <ul>
            {warnings.map((warning, i) => (
              <li key={`${warning.frame_id}-${i}`}>{warningText(job, warning, t)}</li>
            ))}
          </ul>
        </details>
      )}
    </>
  );
}
/** A job as the progress list shows it: what it is, how far it got, and what came of it. */
export function describeJob(job: JobRow, t: TFunction, language: string): ProgressItem {
  const parts: string[] = [];
  if (job.state === 'done' && job.imported !== undefined) {
    parts.push(t('jobs.imported', { count: job.imported }));
  }
  if (job.skipped && job.skipped.length > 0) {
    parts.push(t('jobs.skipped', { count: job.skipped.length }));
  }
  if (job.warnings && job.warnings.length > 0) {
    parts.push(t('jobs.warnings', { count: job.warnings.length }));
  }
  if (job.state === 'done' && job.exported !== undefined) {
    parts.push(t('jobs.exported', { count: job.exported.length }));
  }
  if (job.failed && job.failed.length > 0) {
    parts.push(t('jobs.exportFailed', { count: job.failed.length }));
  }
  if (job.state === 'done' && job.verified) {
    parts.push(
      job.verified.problems.length > 0
        ? t('jobs.verifyProblems', { count: job.verified.problems.length })
        : t('jobs.verifyClean'),
    );
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
        : job.kind === 'verify'
          ? t('jobs.verify')
          : t('jobs.import', { roll: job.rollName }),
    fraction: job.fraction,
    state: job.state,
    stateLabel: t(`jobs.${job.state}`),
    detail:
      parts.length > 0
        ? new Intl.ListFormat(language, { style: 'narrow', type: 'conjunction' }).format(parts)
        : undefined,
    extra: <JobDetails job={job} t={t} />,
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
