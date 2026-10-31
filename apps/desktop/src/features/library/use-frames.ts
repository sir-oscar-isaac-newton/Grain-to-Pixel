import { useCallback, useEffect, useReducer, useRef } from 'react';
import type { FramePage, FrameSummary } from '../../shared/ipc';
import { messageOf } from './library-store';

/** Frames fetched per request. */
export const PAGE_SIZE = 200;

export type PageFetcher = (page: number, pageSize: number) => Promise<FramePage>;

export interface Frames {
  /** Frames matching the query, over all pages. */
  total: number;
  /** True until the first page has arrived. */
  loading: boolean;
  error: string | null;
  /** The frame at a position, or undefined while its page is not loaded. */
  frameAt: (index: number) => FrameSummary | undefined;
  /** Starts loading the pages that cover positions `start` to `end` (inclusive). */
  ensure: (start: number, end: number) => void;
  /** Loads positions `start` to `end` (inclusive) and returns them in order. */
  load: (start: number, end: number) => Promise<FrameSummary[]>;
}

function pageRange(start: number, end: number): number[] {
  const first = Math.floor(Math.max(0, start) / PAGE_SIZE);
  const last = Math.floor(Math.max(start, end) / PAGE_SIZE);
  return Array.from({ length: last - first + 1 }, (_, n) => first + n);
}

/**
 * The frames of a query as sparse pages, loaded as the grid scrolls. `key` names the query: a new
 * key starts over; a new `revision` with the same key reloads the pages already held in place, so
 * the grid does not flash empty when frames change underneath it.
 */
export function useFrames(fetchPage: PageFetcher, key: string, revision: number): Frames {
  const [, rerender] = useReducer((n: number) => n + 1, 0);
  const pages = useRef(new Map<number, FrameSummary[]>());
  const total = useRef(0);
  const loading = useRef(true);
  const error = useRef<string | null>(null);
  const inflight = useRef(new Set<number>());
  const generation = useRef(0);
  const lastKey = useRef<string | null>(null);
  const fetchRef = useRef(fetchPage);
  fetchRef.current = fetchPage;

  /** Fetches the given pages and merges them in, unless the query changed meanwhile. */
  const fetchPages = useCallback(async (indexes: number[], gen: number, replace: boolean) => {
    const wanted = indexes.filter((i) => replace || !inflight.current.has(i));
    wanted.forEach((i) => inflight.current.add(i));
    try {
      const results = await Promise.all(wanted.map((i) => fetchRef.current(i, PAGE_SIZE)));
      if (gen !== generation.current) return;
      if (replace) pages.current = new Map();
      results.forEach((page, n) => pages.current.set(wanted[n] ?? 0, page.items));
      total.current = results[0]?.total ?? total.current;
      error.current = null;
    } catch (e) {
      if (gen !== generation.current) return;
      error.current = messageOf(e);
    } finally {
      wanted.forEach((i) => inflight.current.delete(i));
      if (gen === generation.current) {
        loading.current = false;
        rerender();
      }
    }
  }, []);

  useEffect(() => {
    generation.current += 1;
    const gen = generation.current;
    const sameQuery = lastKey.current === key;
    lastKey.current = key;
    // Same query: reload the pages shown now, in place. A new query starts empty.
    const held = sameQuery ? [...pages.current.keys()] : [];
    if (!sameQuery) {
      pages.current = new Map();
      total.current = 0;
      loading.current = true;
      error.current = null;
      inflight.current.clear();
      rerender();
    }
    const indexes = [...new Set([0, ...held])].sort((a, b) => a - b);
    void fetchPages(indexes, gen, true);
  }, [key, revision, fetchPages]);

  const ensure = useCallback(
    (start: number, end: number) => {
      const missing = pageRange(start, end).filter(
        (p) => !pages.current.has(p) && !inflight.current.has(p),
      );
      if (missing.length > 0) void fetchPages(missing, generation.current, false);
    },
    [fetchPages],
  );

  const load = useCallback(async (start: number, end: number) => {
    const gen = generation.current;
    const missing = pageRange(start, end).filter((p) => !pages.current.has(p));
    if (missing.length > 0) {
      const fetched = await Promise.all(missing.map((p) => fetchRef.current(p, PAGE_SIZE)));
      if (gen === generation.current) {
        fetched.forEach((page, n) => pages.current.set(missing[n] ?? 0, page.items));
        rerender();
      }
    }
    const out: FrameSummary[] = [];
    for (let i = Math.max(0, start); i <= Math.min(end, total.current - 1); i++) {
      const frame = pages.current.get(Math.floor(i / PAGE_SIZE))?.[i % PAGE_SIZE];
      if (frame) out.push(frame);
    }
    return out;
  }, []);

  const frameAt = useCallback(
    (index: number) => pages.current.get(Math.floor(index / PAGE_SIZE))?.[index % PAGE_SIZE],
    [],
  );

  return {
    total: total.current,
    loading: loading.current,
    error: error.current,
    frameAt,
    ensure,
    load,
  };
}
