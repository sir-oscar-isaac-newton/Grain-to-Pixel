import { describe, expect, it } from 'vitest';
import { EditHistory, HISTORY_LIMIT, MERGE_WINDOW_MS } from './history';

describe('EditHistory', () => {
  it('starts with nothing to undo or redo', () => {
    const h = new EditHistory<number>();
    expect(h.canUndo).toBe(false);
    expect(h.canRedo).toBe(false);
    expect(h.undo(1)).toBeNull();
    expect(h.redo(1)).toBeNull();
  });

  it('records one step for each change that is not a drag', () => {
    const h = new EditHistory<number>();
    expect(h.record(0, 1, false)).toBe(true);
    expect(h.record(1, 2, false)).toBe(true);
    expect(h.depth).toBe(2);
    expect(h.undo(2)).toBe(1);
    expect(h.undo(1)).toBe(0);
    expect(h.undo(0)).toBeNull();
  });

  it('folds a drag and its release into one step that goes back to before the first update', () => {
    const h = new EditHistory<number>();
    expect(h.record(0, 1, true)).toBe(false);
    expect(h.record(1, 2, true)).toBe(false);
    expect(h.record(2, 3, true)).toBe(false);
    expect(h.canUndo).toBe(false);
    expect(h.record(3, 3, false)).toBe(true);
    expect(h.depth).toBe(1);
    expect(h.undo(3)).toBe(0);
  });

  it('starts a new drag after the release', () => {
    const h = new EditHistory<number>();
    h.record(0, 1, true);
    h.record(1, 2, false);
    h.record(2, 3, true);
    h.record(3, 4, false);
    expect(h.depth).toBe(2);
    expect(h.undo(4)).toBe(2);
    expect(h.undo(2)).toBe(0);
  });

  it('does not record a change that leaves the recipe as it was', () => {
    const h = new EditHistory<{ a: number }>();
    expect(h.record({ a: 1 }, { a: 1 }, false)).toBe(false);
    // A drag that ends where it began is not a step either.
    h.record({ a: 1 }, { a: 2 }, true);
    expect(h.record({ a: 2 }, { a: 1 }, false)).toBe(false);
    expect(h.canUndo).toBe(false);
  });

  it('keeps at least 50 steps and drops the oldest beyond the limit', () => {
    expect(HISTORY_LIMIT).toBeGreaterThanOrEqual(50);
    const h = new EditHistory<number>();
    for (let i = 0; i < HISTORY_LIMIT + 10; i += 1) h.record(i, i + 1, false);
    expect(h.depth).toBe(HISTORY_LIMIT);
    let current = HISTORY_LIMIT + 10;
    for (let i = 0; i < HISTORY_LIMIT; i += 1) {
      const previous = h.undo(current);
      expect(previous).not.toBeNull();
      current = previous ?? current;
    }
    // The ten oldest are gone: the furthest back is the recipe from before step 11.
    expect(current).toBe(10);
    expect(h.undo(current)).toBeNull();
  });

  it('redoes what was undone, in order', () => {
    const h = new EditHistory<number>();
    h.record(0, 1, false);
    h.record(1, 2, false);
    expect(h.undo(2)).toBe(1);
    expect(h.undo(1)).toBe(0);
    expect(h.canRedo).toBe(true);
    expect(h.redo(0)).toBe(1);
    expect(h.redo(1)).toBe(2);
    expect(h.redo(2)).toBeNull();
    expect(h.undo(2)).toBe(1);
  });

  it('discards the redo steps when a new change is made after an undo', () => {
    const h = new EditHistory<number>();
    h.record(0, 1, false);
    h.record(1, 2, false);
    h.undo(2);
    expect(h.canRedo).toBe(true);
    h.record(1, 5, false);
    expect(h.canRedo).toBe(false);
    expect(h.redo(5)).toBeNull();
  });

  it('does not record an undo or a redo as a step of its own', () => {
    const h = new EditHistory<number>();
    h.record(0, 1, false);
    expect(h.undo(1)).toBe(0);
    expect(h.depth).toBe(0);
    expect(h.redo(0)).toBe(1);
    expect(h.depth).toBe(1);
  });

  it('forgets everything when cleared, including a drag in progress', () => {
    const h = new EditHistory<number>();
    h.record(0, 1, false);
    h.undo(1);
    h.record(5, 6, true);
    h.clear();
    expect(h.canUndo).toBe(false);
    expect(h.canRedo).toBe(false);
    // The release of the cleared drag takes nothing over from it.
    h.record(7, 8, false);
    expect(h.undo(8)).toBe(7);
  });
});

describe('EditHistory merging a reset with the step before', () => {
  type R = { a: number; b: { c: number; d: number } };
  const r = (a: number, c = 0, d = 0): R => ({ a, b: { c, d } });
  let time = 0;
  const make = () => {
    time = 1000;
    return new EditHistory<R>(HISTORY_LIMIT, () => time);
  };

  it('replaces a recent step of the same field, so one undo goes back to before both', () => {
    const h = make();
    h.record(r(0), r(5), false);
    time += 100;
    expect(h.record(r(5), r(2), false, true)).toBe(true);
    expect(h.depth).toBe(1);
    expect(h.undo(r(2))).toEqual(r(0));
    expect(h.canUndo).toBe(false);
    expect(h.redo(r(0))).toEqual(r(2));
  });

  it('merges within the window and not after it', () => {
    const h = make();
    h.record(r(0), r(5), false);
    time += MERGE_WINDOW_MS;
    h.record(r(5), r(2), false, true);
    expect(h.depth).toBe(1);

    const late = make();
    late.record(r(0), r(5), false);
    time += MERGE_WINDOW_MS + 1;
    late.record(r(5), r(2), false, true);
    expect(late.depth).toBe(2);
  });

  it('drops the step when the merged change leaves the recipe as it was', () => {
    const h = make();
    h.record(r(0), r(5), false);
    expect(h.record(r(5), r(0), false, true)).toBe(true);
    expect(h.depth).toBe(0);
    expect(h.canUndo).toBe(false);
    expect(h.canRedo).toBe(false);
  });

  it('merges into a step of a nested field, and when the reset changes more fields', () => {
    const h = make();
    h.record(r(0, 0, 0), r(0, 3, 0), false);
    h.record(r(0, 3, 0), r(0, 1, 0), false, true);
    expect(h.depth).toBe(1);
    h.record(r(0, 1, 0), r(0, 1, 4), false);
    // The reset puts back two fields: the earlier step's one is among them.
    h.record(r(0, 1, 4), r(0, 2, 0), false, true);
    expect(h.depth).toBe(2);
    expect(h.undo(r(0, 2, 0))).toEqual(r(0, 1, 0));
  });

  it('is a step of its own after a change to another field', () => {
    const h = make();
    h.record(r(0), r(5), false);
    h.record(r(5), r(5, 1), false, true);
    expect(h.depth).toBe(2);
    expect(h.undo(r(5, 1))).toEqual(r(5));
  });

  it('is a step of its own when the earlier step changed a field the reset leaves alone', () => {
    const h = make();
    h.record(r(0), r(5, 1), false);
    h.record(r(5, 1), r(5, 0), false, true);
    expect(h.depth).toBe(2);
  });

  it('is a step of its own without a request to merge', () => {
    const h = make();
    h.record(r(0), r(5), false);
    h.record(r(5), r(2), false);
    expect(h.depth).toBe(2);
  });

  it('is a step of its own after an undo, a redo, a clear or when there is no step', () => {
    const h = make();
    h.record(r(0), r(5), false, true);
    expect(h.depth).toBe(1);
    h.undo(r(5));
    h.redo(r(0));
    h.record(r(5), r(2), false, true);
    expect(h.depth).toBe(2);
    h.clear();
    expect(h.record(r(0), r(1), false, true)).toBe(true);
    expect(h.depth).toBe(1);
    h.clear();
    h.record(r(0), r(1), false);
    h.undo(r(1));
    expect(h.record(r(0), r(3), false, true)).toBe(true);
    expect(h.depth).toBe(1);
  });

  it('discards the redo steps like any new change', () => {
    const h = make();
    h.record(r(0), r(5), false);
    h.record(r(5), r(6), false);
    h.undo(r(6));
    expect(h.canRedo).toBe(true);
    h.record(r(5), r(7), false);
    h.record(r(7), r(5), false, true);
    expect(h.canRedo).toBe(false);
  });

  it('keeps a drag folded with its release as one step that a reset can then replace', () => {
    const h = make();
    h.record(r(0), r(1), true);
    h.record(r(1), r(2), true);
    h.record(r(2), r(3), false);
    expect(h.depth).toBe(1);
    h.record(r(3), r(0), false, true);
    expect(h.depth).toBe(0);
  });

  it('does not merge while a drag is in progress', () => {
    const h = make();
    h.record(r(0), r(5), false);
    h.record(r(5), r(6), true, true);
    expect(h.depth).toBe(1);
  });
});
