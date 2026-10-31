import { useCallback, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { formatBytes } from '../../shared/format';
import { frameList } from '../../shared/ipc';
import { FirstRunDialog } from './FirstRunDialog';
import { FrameGrid } from './FrameGrid';
import { JobsPanel } from './JobsPanel';
import styles from './library.module.css';
import { useLibraryStore } from './library-store';
import { RollSidebar } from './RollSidebar';
import { useLibraryEvents } from './use-library-events';

/**
 * The library: a first-run dialog until a library is open, then the rolls on the left and the
 * frames of the chosen roll (or all of them) as a grid of thumbnails.
 */
export function LibraryView() {
  const { t, i18n } = useTranslation();
  const phase = useLibraryStore((s) => s.phase);
  const library = useLibraryStore((s) => s.library);
  const rollId = useLibraryStore((s) => s.rollId);
  const rolls = useLibraryStore((s) => s.rolls);
  const selectedCount = useLibraryStore((s) => s.selected.size);
  const notice = useLibraryStore((s) => s.notice);
  const setNotice = useLibraryStore((s) => s.setNotice);

  useEffect(() => {
    void useLibraryStore.getState().start();
  }, []);
  useLibraryEvents(phase === 'ready');

  const fetchPage = useCallback(
    (page: number, pageSize: number) => frameList({ roll_id: rollId, page, page_size: pageSize }),
    [rollId],
  );
  const rollHasName = rolls.some((r) => r.id === rollId);

  return (
    <section aria-labelledby="library-title" className={styles.view}>
      <h2 id="library-title" className={styles.title}>
        {t('library.title')}
      </h2>
      {phase === 'loading' && <p role="status">{t('library.loading')}</p>}
      {phase === 'needs-library' && <FirstRunDialog />}
      {phase === 'ready' && library && (
        <div className={styles.layout}>
          <RollSidebar />
          <div className={styles.content}>
            <div className={styles.toolbar}>
              <span>{t('library.location.label', { name: library.name })}</span>
              {typeof library.free_bytes === 'number' && (
                <span className={styles.muted}>
                  {t('library.location.free', {
                    size: formatBytes(library.free_bytes, i18n.language),
                  })}
                </span>
              )}
              <span className={styles.muted} aria-live="polite">
                {selectedCount > 0 && t('library.grid.selected', { count: selectedCount })}
              </span>
            </div>
            {library.read_only && (
              <p role="status" className={styles.banner}>
                {t('library.readOnly')}
              </p>
            )}
            {notice && (
              <div role="status" className={styles.banner}>
                <span>{t(notice.key, notice.values)}</span>
                <button type="button" onClick={() => setNotice(null)}>
                  {t('library.notice.dismiss')}
                </button>
              </div>
            )}
            <JobsPanel />
            <FrameGrid
              fetchPage={fetchPage}
              queryKey={rollId ?? 'all'}
              showRoll={rollId === null}
              emptyText={rollHasName ? t('library.emptyRoll') : t('library.emptyAll')}
            />
          </div>
        </div>
      )}
    </section>
  );
}
