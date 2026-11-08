import { useTranslation } from 'react-i18next';
import { useLibraryStore } from '../library/library-store';
import { useLibraryEvents } from '../library/use-library-events';
import { CaptureSection } from './CaptureSection';
import { FilesSection } from './FilesSection';
import { ScannerSection } from './ScannerSection';
import styles from './import.module.css';

/** Bringing frames in: scanning a roll, and camera files shot as red, green and blue exposures. */
export function ImportView() {
  const { t } = useTranslation();
  const ready = useLibraryStore((s) => s.phase === 'ready');
  // The import job started here reports through the job events.
  useLibraryEvents(true);

  return (
    <section aria-labelledby="import-title" className={styles.view}>
      <h2 id="import-title" className={styles.title}>
        {t('import.title')}
      </h2>
      {ready ? (
        <>
          <ScannerSection />
          <FilesSection />
          <CaptureSection />
        </>
      ) : (
        <p role="status">{t('import.needsLibrary')}</p>
      )}
    </section>
  );
}
