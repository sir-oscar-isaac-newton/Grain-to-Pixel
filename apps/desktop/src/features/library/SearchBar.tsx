import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { TagChips } from '@gtp/ui';
import styles from './library.module.css';
import { useLibraryStore } from './library-store';
import { NO_FILTERS, activeFilterCount, isFiltered, type Filters } from './search-query';

/** Typing pauses this long before the grid searches. */
export const SEARCH_DELAY_MS = 250;

/**
 * The search box, and under it a folding row of filters: tags, film, camera, ISO and the shooting
 * dates. Film and camera suggest what the rolls hold; tags suggest the library's tags. The grid
 * follows the filters held in the store; `total` is the number of frames found.
 */
export function SearchBar({ total }: { total: number | null }) {
  const { t } = useTranslation();
  const ids = { film: useId(), camera: useId(), panel: useId() };
  const filters = useLibraryStore((s) => s.filters);
  const setFilters = useLibraryStore((s) => s.setFilters);
  const rolls = useLibraryStore((s) => s.rolls);
  const tags = useLibraryStore((s) => s.tags);
  const setNotice = useLibraryStore((s) => s.setNotice);

  const [open, setOpen] = useState(false);
  const [text, setText] = useState(filters.text);
  const latest = useRef(filters);
  latest.current = filters;

  // The text waits for a pause in typing; the other filters apply at once.
  useEffect(() => {
    if (text === latest.current.text) return;
    const timer = setTimeout(() => setFilters({ ...latest.current, text }), SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [text, setFilters]);
  // Cleared from elsewhere (a new library): the box follows.
  useEffect(() => {
    setText(filters.text);
  }, [filters.text]);

  const earlier = useMemo(() => {
    const distinct = (pick: (r: (typeof rolls)[number]) => string | null) =>
      [...new Set(rolls.map(pick).filter((v): v is string => !!v))].sort();
    return { film: distinct((r) => r.film_stock), camera: distinct((r) => r.camera) };
  }, [rolls]);

  const change = (patch: Partial<Filters>) => setFilters({ ...filters, ...patch });
  const chosen = filters.tagIds.flatMap((id) => tags.filter((tag) => tag.id === id));
  const active = activeFilterCount(filters);

  const addTag = (name: string) => {
    const key = name.toLowerCase();
    const tag = tags.find((candidate) => candidate.name.toLowerCase() === key);
    if (!tag) {
      setNotice({ key: 'library.search.noSuchTag', values: { name } });
      return;
    }
    if (!filters.tagIds.includes(tag.id)) change({ tagIds: [...filters.tagIds, tag.id] });
  };

  return (
    <div className={styles.search}>
      <div className={styles.searchRow} role="search">
        <input
          type="search"
          className={styles.searchInput}
          aria-label={t('library.search.label')}
          placeholder={t('library.search.placeholder')}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <button
          type="button"
          aria-expanded={open}
          aria-controls={ids.panel}
          onClick={() => setOpen(!open)}
        >
          {active > 0
            ? t('library.search.filtersActive', { count: active })
            : t('library.search.filters')}
        </button>
        <button
          type="button"
          disabled={!isFiltered(filters) && text === ''}
          onClick={() => {
            setText('');
            setFilters(NO_FILTERS);
          }}
        >
          {t('library.search.clear')}
        </button>
        <span className={styles.muted} role="status">
          {total !== null && isFiltered(filters) && t('library.search.found', { count: total })}
        </span>
      </div>

      <div id={ids.panel} hidden={!open} className={styles.filters}>
        <div className={styles.filterGrid}>
          <label className={styles.field}>
            <span>{t('library.search.film')}</span>
            <input
              type="text"
              list={ids.film}
              value={filters.film}
              onChange={(e) => change({ film: e.target.value })}
            />
            <datalist id={ids.film}>
              {earlier.film.map((v) => (
                <option key={v} value={v} />
              ))}
            </datalist>
          </label>
          <label className={styles.field}>
            <span>{t('library.search.camera')}</span>
            <input
              type="text"
              list={ids.camera}
              value={filters.camera}
              onChange={(e) => change({ camera: e.target.value })}
            />
            <datalist id={ids.camera}>
              {earlier.camera.map((v) => (
                <option key={v} value={v} />
              ))}
            </datalist>
          </label>
          <label className={styles.field}>
            <span>{t('library.search.iso')}</span>
            <input
              type="text"
              inputMode="numeric"
              value={filters.iso}
              onChange={(e) => change({ iso: e.target.value.replace(/\D/g, '') })}
            />
          </label>
          <label className={styles.field}>
            <span>{t('library.search.from')}</span>
            <input
              type="text"
              value={filters.from}
              placeholder="2026-07"
              onChange={(e) => change({ from: e.target.value })}
            />
          </label>
          <label className={styles.field}>
            <span>{t('library.search.to')}</span>
            <input
              type="text"
              value={filters.to}
              placeholder="2026-08-02"
              onChange={(e) => change({ to: e.target.value })}
            />
          </label>
        </div>
        <TagChips
          label={t('library.search.tags')}
          tags={chosen}
          emptyText={t('library.search.noTagsChosen')}
          removeLabel={(name) => t('library.search.removeTag', { name })}
          onRemove={(tag) => change({ tagIds: filters.tagIds.filter((id) => id !== tag.id) })}
          onAdd={addTag}
          addLabel={t('library.search.addTag')}
          addButtonLabel={t('library.search.addTagButton')}
          suggestions={tags
            .filter((tag) => !filters.tagIds.includes(tag.id))
            .map((tag) => tag.name)}
        />
        <p className={styles.hint}>{t('library.search.dateHint')}</p>
      </div>
    </div>
  );
}
