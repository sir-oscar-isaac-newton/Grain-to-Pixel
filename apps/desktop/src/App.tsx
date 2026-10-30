import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './App.module.css';
import { useUiStore, type View } from './app/store';
import { DevelopView } from './features/develop/DevelopView';
import { ImportView } from './features/import/ImportView';
import { LibraryView } from './features/library/LibraryView';
import { crashNotesTake, libraryInfo } from './shared/ipc';

const VIEWS: readonly View[] = ['library', 'develop', 'import'];

export function App() {
  const { t } = useTranslation();
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);
  const [version, setVersion] = useState<string | null>(null);
  const [crashed, setCrashed] = useState(false);

  useEffect(() => {
    let alive = true;
    crashNotesTake()
      .then((notes) => {
        if (alive) setCrashed(notes.length > 0);
      })
      .catch(() => {
        // No backend (tests, plain browser): nothing to report.
      });
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    let alive = true;
    libraryInfo()
      .then((info) => {
        if (alive) setVersion(info.app_version);
      })
      .catch(() => {
        // Outside the Tauri shell (tests, plain browser) there is no backend.
        if (alive) setVersion(null);
      });
    return () => {
      alive = false;
    };
  }, []);

  return (
    <div className={styles.shell}>
      <header className={styles.header}>
        <h1 className={styles.title}>{t('app.title')}</h1>
        <nav aria-label={t('nav.label')} className={styles.nav}>
          {VIEWS.map((v) => (
            <button
              key={v}
              type="button"
              className={styles.tab}
              aria-current={view === v ? 'page' : undefined}
              onClick={() => setView(v)}
            >
              {t(`nav.${v}`)}
            </button>
          ))}
        </nav>
      </header>
      <main className={styles.main}>
        {crashed && (
          <div role="status" className={styles.notice}>
            <span>{t('crash.notice')}</span>
            <button type="button" onClick={() => setCrashed(false)}>
              {t('crash.dismiss')}
            </button>
          </div>
        )}
        {view === 'library' && <LibraryView />}
        {view === 'develop' && <DevelopView />}
        {view === 'import' && <ImportView />}
      </main>
      <footer className={styles.footer}>
        {t('footer.version', { version: version ?? t('footer.unknown') })}
      </footer>
    </div>
  );
}
