import { useCallback, useMemo } from 'react';
import { frameList, search } from '../../shared/ipc';
import { useLibraryStore } from './library-store';
import { isFiltered, toQuery } from './search-query';
import type { PageFetcher } from './use-frames';

/** Frames read per request when a whole view is read (Select rejected). */
const WHOLE_PAGE_SIZE = 500;
/** A view is read in pages of {@link WHOLE_PAGE_SIZE}; no real library needs more than this. */
const MAX_PAGES = 20;

export interface LibraryQuery {
  /** Loads one page of the frames the Library shows now. */
  fetchPage: PageFetcher;
  /** Names the query: a new key starts a list over. */
  queryKey: string;
  /** True when the search box or a filter narrows the frames down. */
  filtered: boolean;
  /** The ids of every rejected frame of the current view (roll, search and tag filters). */
  fetchRejectedIds: () => Promise<string[]>;
}

/**
 * The frames the Library shows: the chosen roll (or all) in order, or the search results, best
 * match first, when something is typed or filtered. The Develop view's filmstrip uses the same
 * query, so it lists the frames the grid did.
 */
export function useLibraryQuery(): LibraryQuery {
  const rollId = useLibraryStore((s) => s.rollId);
  const filters = useLibraryStore((s) => s.filters);
  const flagFilter = useLibraryStore((s) => s.flagFilter);
  const filtered = isFiltered(filters);
  const query = useMemo(() => toQuery(filters, rollId, flagFilter), [filters, rollId, flagFilter]);
  const flag = flagFilter === 'all' ? null : flagFilter;

  const fetchPage = useCallback(
    (page: number, pageSize: number) =>
      filtered
        ? search(query, page, pageSize)
        : frameList({ roll_id: rollId, flag, page, page_size: pageSize }),
    [filtered, query, rollId, flag],
  );
  const queryKey = filtered ? JSON.stringify(query) : `${rollId ?? 'all'}|${flagFilter}`;

  const fetchRejectedIds = useCallback(async () => {
    // The view's own flag filter still applies: with Picked or Unflagged chosen no frame of it
    // is rejected.
    if (flagFilter === 'pick' || flagFilter === 'none') return [];
    const ids: string[] = [];
    for (let page = 0; page < MAX_PAGES; page += 1) {
      const result = filtered
        ? await search({ ...query, flag: 'reject' }, page, WHOLE_PAGE_SIZE)
        : await frameList({ roll_id: rollId, flag: 'reject', page, page_size: WHOLE_PAGE_SIZE });
      ids.push(...result.items.map((frame) => frame.id));
      if (ids.length >= result.total || result.items.length === 0) break;
    }
    return ids;
  }, [filtered, query, rollId, flagFilter]);

  return { fetchPage, queryKey, filtered, fetchRejectedIds };
}
