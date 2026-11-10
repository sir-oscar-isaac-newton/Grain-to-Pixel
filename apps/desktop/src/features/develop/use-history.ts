import { useCallback, useReducer, useRef } from 'react';
import { EditHistory, HISTORY_LIMIT } from './history';

export interface HistoryControls<T> {
  canUndo: boolean;
  canRedo: boolean;
  /** Notes a change (see {@link EditHistory.record}); `merge` for a slider reset. */
  record: (before: T, after: T, dragging: boolean, merge?: boolean) => void;
  /** The recipe one step back, or null. */
  undo: (current: T) => T | null;
  /** The recipe one step forward, or null. */
  redo: (current: T) => T | null;
  clear: () => void;
}

/** An {@link EditHistory} that keeps the component showing whether there is a step to undo or redo. */
export function useHistory<T>(
  limit: number = HISTORY_LIMIT,
  now?: () => number,
): HistoryControls<T> {
  const history = useRef<EditHistory<T> | null>(null);
  history.current ??= new EditHistory<T>(limit, now);
  const [, refresh] = useReducer((n: number) => n + 1, 0);
  const h = history.current;
  const record = useCallback(
    (before: T, after: T, dragging: boolean, merge = false) => {
      if (h.record(before, after, dragging, merge)) refresh();
    },
    [h],
  );
  const undo = useCallback(
    (current: T) => {
      const result = h.undo(current);
      refresh();
      return result;
    },
    [h],
  );
  const redo = useCallback(
    (current: T) => {
      const result = h.redo(current);
      refresh();
      return result;
    },
    [h],
  );
  const clear = useCallback(() => {
    h.clear();
    refresh();
  }, [h]);
  return { canUndo: h.canUndo, canRedo: h.canRedo, record, undo, redo, clear };
}
