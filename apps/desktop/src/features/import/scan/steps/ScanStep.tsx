import { ProgressList } from '@gtp/ui';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { jobCancel } from '../../../../shared/ipc';
import { describeJob } from '../../../library/JobsPanel';
import { useLibraryStore } from '../../../library/library-store';
import styles from '../../import.module.css';
import { StepSection } from '../../capture/steps/StepSection';
import { useScan } from '../ScanProvider';

/** Step "Scan": start the scan, follow its progress, cancel it. Finish opens the roll in the Library. */
export function ScanStep() {
  const { t, i18n } = useTranslation();
  const scan = useScan();
  const { ensureDevices, first, last } = scan;
  const allJobs = useLibraryStore((s) => s.jobs);
  const dismissJob = useLibraryStore((s) => s.dismissJob);
  const jobs = allJobs.filter((j) => j.kind === 'scan');
  useEffect(() => {
    ensureDevices();
  }, [ensureDevices]);

  return (
    <StepSection id="import-scan" heading={t('import.steps.scan.heading')}>
      <p className={styles.muted}>{t('import.steps.scan.intro')}</p>
      {scan.complete && <p className={styles.muted}>{t('import.scanner.complete')}</p>}
      <div className={styles.buttons}>
        <button
          type="button"
          disabled={
            scan.locked ||
            scan.starting ||
            scan.previewing ||
            !scan.valid ||
            !scan.roll ||
            !scan.device ||
            scan.complete
          }
          onClick={() => void scan.start()}
        >
          {scan.valid
            ? t('import.scanner.start', { count: Number(last) - Number(first) + 1, first, last })
            : t('import.scanner.startPlain')}
        </button>
        {scan.previewing && (
          <button type="button" onClick={scan.cancelPreview}>
            {t('import.scanner.previewCancel')}
          </button>
        )}
      </div>
      {scan.previewing && <p className={styles.muted}>{t('import.scanner.previewing')}</p>}
      {scan.startError && <p role="alert">{scan.startError}</p>}
      <ProgressList
        items={jobs.map((job) => describeJob(job, t, i18n.language))}
        label={t('jobs.label')}
        cancelLabel={t('jobs.cancel')}
        dismissLabel={t('jobs.dismiss')}
        onCancel={(jobId) => void jobCancel(jobId)}
        onDismiss={dismissJob}
      />
    </StepSection>
  );
}
