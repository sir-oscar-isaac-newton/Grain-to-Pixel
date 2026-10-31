import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useUiStore } from '../../app/store';
import { formatBytes } from '../../shared/format';
import { FirstRunDialog } from './FirstRunDialog';
import { FrameGrid } from './FrameGrid';
import { JobsPanel } from './JobsPanel';
import styles from './library.module.css';
import { useLibraryStore } from './library-store';
import { RollSidebar } from './RollSidebar';
import { SearchBar } from './SearchBar';
import { SelectionTags } from './SelectionTags';
import { useLibraryEvents } from './use-library-events';
import { useLibraryQuery } from './use-library-query';

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
    // Coming back from the Develop view, the library is open already and the roll and filters
    // you left are kept.
    if (useLibraryStore.getState().phase !== 'ready') void useLibraryStore.getState().start();
  }, []);
  useLibraryEvents(phase === 'ready');

  const openInDevelop = useUiStore((s) => s.openInDevelop);
  const [found, setFound] = useState<number | null>(null);

  // Nothing typed: the roll's frames in order. Otherwise the search, best match first.
  const { fetchPage, queryKey, filtered } = useLibraryQuery();
  const rollHasName = rolls.some((r) => r.id === rollId);
  const emptyText = filtered
    ? t('library.search.noMatches')
    : rollHasName
      ? t('library.emptyRoll')
      : t('library.emptyAll');

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
            <SearchBar total={found} />
            {selectedCount > 0 && <SelectionTags />}
            <FrameGrid
              fetchPage={fetchPage}
              queryKey={queryKey}
              showRoll={rollId === null}
              emptyText={emptyText}
              onTotal={setFound}
              onOpen={(frame, index) => openInDevelop(frame.id, index)}
            />
          </div>
        </div>
      )}
    </section>
  );
}
