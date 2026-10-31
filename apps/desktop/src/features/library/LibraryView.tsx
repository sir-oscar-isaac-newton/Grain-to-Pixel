import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { formatBytes } from '../../shared/format';
import { frameList, search } from '../../shared/ipc';
import { FirstRunDialog } from './FirstRunDialog';
import { FrameGrid } from './FrameGrid';
import { JobsPanel } from './JobsPanel';
import styles from './library.module.css';
import { useLibraryStore } from './library-store';
import { RollSidebar } from './RollSidebar';
import { SearchBar } from './SearchBar';
import { isFiltered, toQuery } from './search-query';
import { SelectionTags } from './SelectionTags';
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

  const filters = useLibraryStore((s) => s.filters);
  const filtered = isFiltered(filters);
  const query = useMemo(() => toQuery(filters, rollId), [filters, rollId]);
  const [found, setFound] = useState<number | null>(null);

  // Nothing typed: the roll's frames in order. Otherwise the search, best match first.
  const fetchPage = useCallback(
    (page: number, pageSize: number) =>
      filtered
        ? search(query, page, pageSize)
        : frameList({ roll_id: rollId, page, page_size: pageSize }),
    [filtered, query, rollId],
  );
  const queryKey = filtered ? JSON.stringify(query) : (rollId ?? 'all');
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
            />
          </div>
        </div>
      )}
    </section>
  );
}
