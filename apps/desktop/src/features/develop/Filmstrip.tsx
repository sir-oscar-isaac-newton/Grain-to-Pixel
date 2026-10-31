import { useVirtualizer } from '@tanstack/react-virtual';
import { memo, useEffect, useRef, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { StatusBadge } from '@gtp/ui';
import type { FrameSummary } from '../../shared/ipc';
import type { Frames } from '../library/use-frames';
import { useThumbnail } from '../library/use-thumbnail';
import styles from './develop.module.css';

/** Width of one frame in the strip, with its gap. */
export const ITEM_WIDTH = 104;

export const optionId = (frameId: string) => `filmstrip-${frameId}`;

interface ItemProps {
  frame: FrameSummary;
  index: number;
  current: boolean;
  onOpen: (frame: FrameSummary, index: number) => void;
}

const Item = memo(function Item({ frame, index, current, onOpen }: ItemProps) {
  const { t } = useTranslation();
  const thumbnail = useThumbnail(frame.id, frame.has_thumb);
  return (
    <div
      id={optionId(frame.id)}
      role="option"
      aria-selected={current}
      aria-label={t('library.frameLabel', { n: frame.index_in_roll })}
      tabIndex={-1}
      className={styles.item}
      onClick={() => onOpen(frame, index)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onOpen(frame, index);
        }
      }}
    >
      <div className={styles.itemThumb}>
        {thumbnail.status === 'ready' ? (
          <img src={thumbnail.url} alt="" draggable={false} className={styles.itemImage} />
        ) : thumbnail.status === 'loading' ? (
          <span className={styles.itemLoading} aria-hidden="true" />
        ) : (
          <StatusBadge tone="warn">{t('library.noPreview')}</StatusBadge>
        )}
      </div>
      <span className={styles.itemNumber} aria-hidden="true">
        {frame.index_in_roll}
      </span>
    </div>
  );
});

interface FilmstripProps {
  /** The frames of the Library's current view. */
  frames: Frames;
  /** The open frame. */
  currentId: string | null;
  currentIndex: number;
  onOpen: (frame: FrameSummary, index: number) => void;
  /** Moves to the previous (-1) or next (1) frame. */
  onStep: (delta: -1 | 1) => void;
}

/**
 * The frames of the Library's current view in a row, the open one marked. Only the frames in view
 * exist in the page; the rest load as the strip scrolls. `[` and `]` (handled by the view) and the
 * arrow keys on the strip move through it.
 */
export function Filmstrip({ frames, currentId, currentIndex, onOpen, onStep }: FilmstripProps) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const { total, ensure, frameAt } = frames;
  const virtualizer = useVirtualizer({
    count: total,
    horizontal: true,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ITEM_WIDTH,
    overscan: 4,
  });
  const visible = virtualizer.getVirtualItems();
  const first = visible[0]?.index ?? 0;
  const last = visible[visible.length - 1]?.index ?? 0;

  useEffect(() => {
    if (total > 0) ensure(first, last);
  }, [ensure, total, first, last]);

  // Keep the open frame in view when it changes (also when it is far from the strip's position).
  useEffect(() => {
    if (currentIndex >= 0 && currentIndex < total) {
      virtualizer.scrollToIndex(currentIndex, { align: 'auto' });
    }
  }, [currentIndex, total, virtualizer]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'ArrowLeft') onStep(-1);
    else if (event.key === 'ArrowRight') onStep(1);
    else return;
    event.preventDefault();
  };

  return (
    <div
      ref={scrollRef}
      role="listbox"
      tabIndex={0}
      aria-label={t('develop.filmstrip.label')}
      aria-orientation="horizontal"
      aria-activedescendant={currentId ? optionId(currentId) : undefined}
      className={styles.strip}
      onKeyDown={onKeyDown}
    >
      <div className={styles.stripTrack} style={{ width: virtualizer.getTotalSize() }}>
        {visible.map((item) => {
          const frame = frameAt(item.index);
          return frame ? (
            <div
              key={frame.id}
              className={styles.stripSlot}
              style={{ transform: `translateX(${item.start}px)`, width: ITEM_WIDTH }}
            >
              <Item
                frame={frame}
                index={item.index}
                current={frame.id === currentId}
                onOpen={onOpen}
              />
            </div>
          ) : null;
        })}
      </div>
    </div>
  );
}
