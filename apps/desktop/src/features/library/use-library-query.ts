import { useCallback, useMemo } from 'react';
import { frameList, search } from '../../shared/ipc';
import { useLibraryStore } from './library-store';
import { isFiltered, toQuery } from './search-query';
import type { PageFetcher } from './use-frames';

export interface LibraryQuery {
  /** Loads one page of the frames the Library shows now. */
  fetchPage: PageFetcher;
  /** Names the query: a new key starts a list over. */
  queryKey: string;
  /** True when the search box or a filter narrows the frames down. */
  filtered: boolean;
}

/**
 * The frames the Library shows: the chosen roll (or all) in order, or the search results, best
 * match first, when something is typed or filtered. The Develop view's filmstrip uses the same
 * query, so it lists the frames the grid did.
 */
export function useLibraryQuery(): LibraryQuery {
  const rollId = useLibraryStore((s) => s.rollId);
  const filters = useLibraryStore((s) => s.filters);
  const filtered = isFiltered(filters);
  const query = useMemo(() => toQuery(filters, rollId), [filters, rollId]);

  const fetchPage = useCallback(
    (page: number, pageSize: number) =>
      filtered
        ? search(query, page, pageSize)
        : frameList({ roll_id: rollId, page, page_size: pageSize }),
    [filtered, query, rollId],
  );
  const queryKey = filtered ? JSON.stringify(query) : (rollId ?? 'all');
  return { fetchPage, queryKey, filtered };
}
