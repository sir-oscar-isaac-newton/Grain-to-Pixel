import { useTranslation } from 'react-i18next';
import { useLibraryStore } from '../library/library-store';
import { useLibraryEvents } from '../library/use-library-events';
import { FilesSection } from './FilesSection';
import styles from './import.module.css';

/** Bringing frames in. Today: camera files shot as red, green and blue exposures. */
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
      {ready ? <FilesSection /> : <p role="status">{t('import.needsLibrary')}</p>}
    </section>
  );
}
