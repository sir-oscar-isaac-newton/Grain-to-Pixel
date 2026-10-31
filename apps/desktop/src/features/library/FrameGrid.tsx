import { useVirtualizer } from '@tanstack/react-virtual';
import { StatusBadge } from '@gtp/ui';
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type RefObject,
} from 'react';
import { useTranslation } from 'react-i18next';
import type { FrameSummary } from '../../shared/ipc';
import styles from './library.module.css';
import { CELL_GAP, ROW_HEIGHT, columnsFor, rangeBetween, targetIndex } from './grid-navigation';
import { useLibraryStore } from './library-store';
import { useFrames, type PageFetcher } from './use-frames';
import { useThumbnail } from './use-thumbnail';

/** The width of an element, kept up to date as it is resized. */
function useElementWidth(ref: RefObject<HTMLElement | null>): number {
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const read = () => setWidth(element.clientWidth);
    read();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(read);
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);
  return width;
}

export const cellId = (frameId: string) => `frame-cell-${frameId}`;

interface CellProps {
  frame: FrameSummary;
  index: number;
  selected: boolean;
  focused: boolean;
  rollName: string | null;
}

const FrameCell = memo(function FrameCell({
  frame,
  index,
  selected,
  focused,
  rollName,
}: CellProps) {
  const { t } = useTranslation();
  const thumbnail = useThumbnail(frame.id, frame.has_thumb);
  return (
    <div
      id={cellId(frame.id)}
      role="gridcell"
      aria-selected={selected}
      data-index={index}
      data-focused={focused ? 'true' : undefined}
      className={styles.cell}
    >
      <div className={styles.thumb}>
        {thumbnail.status === 'ready' ? (
          <img src={thumbnail.url} alt="" draggable={false} className={styles.image} />
        ) : thumbnail.status === 'loading' ? (
          <span className={styles.loading} aria-hidden="true" />
        ) : (
          <StatusBadge tone="warn">{t('library.noPreview')}</StatusBadge>
        )}
      </div>
      <div className={styles.caption}>
        <span>{t('library.frameLabel', { n: frame.index_in_roll })}</span>
        {frame.edited_at && <StatusBadge tone="ok">{t('library.grid.edited')}</StatusBadge>}
      </div>
      {rollName && <div className={styles.captionRoll}>{rollName}</div>}
    </div>
  );
});

interface FrameGridProps {
  /** Loads one page of the frames to show. */
  fetchPage: PageFetcher;
  /** Names the query: a new key starts the grid over. */
  queryKey: string;
  /** Shown when there are no frames. */
  emptyText: string;
  /** Show each frame's roll under it (when frames of several rolls are listed). */
  showRoll: boolean;
  /** Told how many frames match, once known (null while a new query loads). */
  onTotal?: (total: number | null) => void;
}

/**
 * Frames as a grid of thumbnails. Only the rows on screen exist in the page, and frames load a page
 * at a time as the grid scrolls. Click, Shift-click and Ctrl/Cmd-click select; the arrow keys,
 * `[` and `]`, Home, End and the page keys move, with Shift extending the selection.
 */
export function FrameGrid({ fetchPage, queryKey, emptyText, showRoll, onTotal }: FrameGridProps) {
  const { t } = useTranslation();
  const revision = useLibraryStore((s) => s.revision);
  const selected = useLibraryStore((s) => s.selected);
  const setSelected = useLibraryStore((s) => s.setSelected);
  const rolls = useLibraryStore((s) => s.rolls);
  const frames = useFrames(fetchPage, queryKey, revision);
  const { total, ensure, load, frameAt } = frames;
  const reportTotal = frames.loading || frames.error ? null : total;
  useEffect(() => onTotal?.(reportTotal), [onTotal, reportTotal]);

  const scrollRef = useRef<HTMLDivElement>(null);
  const width = useElementWidth(scrollRef);
  const columns = columnsFor(width);
  const rowCount = Math.ceil(total / columns);
  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 3,
  });
  const visible = virtualizer.getVirtualItems();
  const firstRow = visible[0]?.index ?? 0;
  const lastRow = visible[visible.length - 1]?.index ?? 0;
  const pageRows = Math.max(
    1,
    Math.floor((scrollRef.current?.clientHeight ?? ROW_HEIGHT) / ROW_HEIGHT),
  );

  // Load what is on screen.
  useEffect(() => {
    if (total > 0) ensure(firstRow * columns, (lastRow + 1) * columns - 1);
  }, [ensure, total, firstRow, lastRow, columns]);

  const [focus, setFocus] = useState(0);
  const anchor = useRef(0);
  useEffect(() => {
    setFocus(0);
    anchor.current = 0;
  }, [queryKey]);

  const rollNames = useMemo(() => new Map(rolls.map((r) => [r.id, r.name])), [rolls]);

  /** Selects positions `a` to `b`, loading their pages first; `keep` stays selected too. */
  const selectBetween = useCallback(
    async (a: number, b: number, keep?: ReadonlySet<string>) => {
      const [lo, hi] = rangeBetween(a, b);
      const list = await load(lo, hi);
      setSelected([...(keep ?? []), ...list.map((f) => f.id)]);
    },
    [load, setSelected],
  );

  const toggle = useCallback(
    (id: string) => {
      const next = new Set(selected);
      if (!next.delete(id)) next.add(id);
      setSelected(next);
    },
    [selected, setSelected],
  );

  const onClick = (event: MouseEvent<HTMLDivElement>) => {
    const cell = (event.target as HTMLElement).closest<HTMLElement>('[data-index]');
    const index = Number(cell?.dataset.index);
    const frame = cell ? frameAt(index) : undefined;
    if (!frame) return;
    setFocus(index);
    if (event.shiftKey) {
      void selectBetween(anchor.current, index);
    } else if (event.ctrlKey || event.metaKey) {
      toggle(frame.id);
      anchor.current = index;
    } else {
      setSelected([frame.id]);
      anchor.current = index;
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const ctrl = event.ctrlKey || event.metaKey;
    if (ctrl && event.key.toLowerCase() === 'a') {
      event.preventDefault();
      if (total > 0) void selectBetween(0, total - 1);
      return;
    }
    if (event.key === 'Escape') {
      setSelected([]);
      return;
    }
    if (event.key === ' ') {
      event.preventDefault();
      const frame = frameAt(focus);
      if (frame) {
        toggle(frame.id);
        anchor.current = focus;
      }
      return;
    }
    const target = targetIndex(event.key, focus, { columns, total, ctrl, pageRows });
    if (target === null) return;
    event.preventDefault();
    setFocus(target);
    virtualizer.scrollToIndex(Math.floor(target / columns));
    if (event.shiftKey) {
      void selectBetween(anchor.current, target);
    } else if (!ctrl) {
      // Moving selects the frame it lands on; with Ctrl the selection is left alone.
      anchor.current = target;
      void load(target, target).then(([frame]) => {
        if (frame) setSelected([frame.id]);
      });
    }
  };

  const focused = frameAt(focus);
  const message = frames.error ? (
    <p role="alert">{t('library.grid.loadError', { message: frames.error })}</p>
  ) : !frames.loading && total === 0 ? (
    <p className={styles.empty}>{emptyText}</p>
  ) : null;
  // The scroller always exists, so its width is measured from the start, also for a roll that is
  // empty now and gets frames later.
  return (
    <div ref={scrollRef} className={styles.scroller}>
      {message}
      {!message && (
        <div
          role="grid"
          tabIndex={0}
          aria-label={t('library.grid.label')}
          aria-rowcount={rowCount}
          aria-colcount={columns}
          aria-multiselectable="true"
          aria-activedescendant={focused ? cellId(focused.id) : undefined}
          aria-busy={frames.loading}
          className={styles.grid}
          style={{ height: virtualizer.getTotalSize() }}
          onClick={onClick}
          onKeyDown={onKeyDown}
        >
          {visible.map((row) => {
            const start = row.index * columns;
            const count = Math.min(columns, total - start);
            return (
              <div
                key={row.key}
                role="row"
                aria-rowindex={row.index + 1}
                className={styles.row}
                style={{
                  transform: `translateY(${row.start}px)`,
                  height: ROW_HEIGHT - CELL_GAP,
                  gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
                }}
              >
                {Array.from({ length: count }, (_, n) => {
                  const index = start + n;
                  const frame = frameAt(index);
                  return frame ? (
                    <FrameCell
                      key={frame.id}
                      frame={frame}
                      index={index}
                      selected={selected.has(frame.id)}
                      focused={index === focus}
                      rollName={showRoll ? (rollNames.get(frame.roll_id) ?? null) : null}
                    />
                  ) : (
                    <div
                      key={`pending-${index}`}
                      role="gridcell"
                      aria-busy="true"
                      className={styles.cell}
                    />
                  );
                })}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
