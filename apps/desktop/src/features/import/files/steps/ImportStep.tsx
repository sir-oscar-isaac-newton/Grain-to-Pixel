import { ProgressList } from '@gtp/ui';
import { useTranslation } from 'react-i18next';
import { jobCancel } from '../../../../shared/ipc';
import { describeJob } from '../../../library/JobsPanel';
import { useLibraryStore } from '../../../library/library-store';
import styles from '../../import.module.css';
import { StepSection } from '../../capture/steps/StepSection';
import { useFiles } from '../FilesProvider';

/** Step "Import": start the import of the files chosen and follow it. Finish opens the roll in the Library. */
export function ImportStep() {
  const { t, i18n } = useTranslation();
  const files = useFiles();
  const allJobs = useLibraryStore((s) => s.jobs);
  const dismissJob = useLibraryStore((s) => s.dismissJob);
  const jobs = allJobs.filter((j) => j.kind === 'import');
  const camera = files.kind === 'camera';

  return (
    <StepSection id="import-import" heading={t('import.steps.import.heading')}>
      <p className={styles.muted}>{t('import.steps.import.intro')}</p>
      {files.count > 0 ? (
        <p role="status" className={styles.summary}>
          {t(camera ? 'import.files.summary' : 'import.files.tiffSummary', { count: files.count })}
        </p>
      ) : (
        !files.started && <p className={styles.muted}>{t('import.steps.import.none')}</p>
      )}
      <div className={styles.buttons}>
        <button
          type="button"
          disabled={files.busy || files.readOnly || files.paths.length === 0 || !files.roll}
          onClick={() => void files.start()}
        >
          {t(camera ? 'import.files.import' : 'import.files.importTiff', { count: files.count })}
        </button>
      </div>
      {files.startError && <p role="alert">{files.startError}</p>}
      {files.started && <p role="status">{t('import.files.started')}</p>}
      <ProgressList
        items={jobs.map((job) => describeJob(job, t, i18n.language))}
        label={t('jobs.label')}
        cancelLabel={t('jobs.cancel')}
        dismissLabel={t('jobs.dismiss')}
        onCancel={(id) => void jobCancel(id)}
        onDismiss={dismissJob}
      />
    </StepSection>
  );
}
