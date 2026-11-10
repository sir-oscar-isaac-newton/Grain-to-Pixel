/** How many steps an {@link EditHistory} keeps; the oldest is dropped beyond it. */
export const HISTORY_LIMIT = 50;

/**
 * A slider reset that comes within this long after a change of the same field takes that change's
 * place instead of being a step of its own. A double click on a slider's track first moves the
 * thumb (the first click) and then resets it (the second); the two together are one undo step.
 */
export const MERGE_WINDOW_MS = 500;

/**
 * The undo history of one frame's recipe: the recipes from before each step (`past`) and the ones
 * undone (`future`). Plain data compared by value, so a change that leaves the recipe as it was is
 * not a step. It knows nothing of React; {@link useHistory} wraps it.
 */
export class EditHistory<T> {
  private past: T[] = [];
  private future: T[] = [];
  /** The recipe from before the first update of the drag in progress, if one is. */
  private gesture: { before: T } | null = null;
  /** The newest step: the recipe it led to and when it was recorded. Null after undo, redo or clear. */
  private last: { after: T; at: number } | null = null;

  /** `now` is the clock (milliseconds) that dates the steps; tests give their own. */
  constructor(
    private readonly limit: number = HISTORY_LIMIT,
    private readonly now: () => number = () => Date.now(),
  ) {}

  get canUndo(): boolean {
    return this.past.length > 0;
  }

  get canRedo(): boolean {
    return this.future.length > 0;
  }

  /** Steps that can be undone. */
  get depth(): number {
    return this.past.length;
  }

  /**
   * Notes that `before` became `after`. While `dragging` nothing is a step yet: the first update
   * remembers `before`. The release (`dragging` false) closes the step with the recipe from before
   * the drag; a change that is not part of a drag is a step of its own. Returns whether a step was
   * recorded or changed.
   *
   * With `merge` (a slider reset) the change takes the place of the newest step when that step was
   * recorded within {@link MERGE_WINDOW_MS} and changed only fields this change changes too: one
   * undo then goes back to before both, and the value in between is not kept. If that leaves the
   * recipe as it was before the step, the step is dropped.
   */
  record(before: T, after: T, dragging: boolean, merge = false): boolean {
    if (dragging) {
      this.gesture ??= { before };
      return false;
    }
    const start = this.gesture ? this.gesture.before : before;
    const folded = this.gesture !== null;
    this.gesture = null;
    if (merge && !folded && this.canMerge(before, after)) {
      const earlier = this.past[this.past.length - 1] as T;
      this.future = [];
      if (same(earlier, after)) {
        this.past.pop();
        this.last = null;
      } else {
        this.last = { after, at: this.last?.at ?? this.now() };
      }
      return true;
    }
    if (same(start, after)) return false;
    this.past.push(start);
    if (this.past.length > this.limit) this.past.shift();
    this.future = [];
    this.last = { after, at: this.now() };
    return true;
  }

  /** The newest step is recent, led to `before`, and changed only fields that `before` to `after` changes. */
  private canMerge(before: T, after: T): boolean {
    const last = this.last;
    if (!last || this.past.length === 0 || !same(last.after, before)) return false;
    if (this.now() - last.at > MERGE_WINDOW_MS) return false;
    const earlier = this.past[this.past.length - 1] as T;
    const now = changedFields(before, after);
    // A path that is a part of the other (a block that appeared, and a value inside it) is the same field.
    const overlap = (x: string, y: string) =>
      x === y || x.startsWith(`${y}.`) || y.startsWith(`${x}.`);
    return [...changedFields(earlier, before)].every((field) =>
      [...now].some((other) => overlap(field, other)),
    );
  }

  /** The recipe one step back, or null when there is none; `current` becomes redoable. */
  undo(current: T): T | null {
    this.gesture = null;
    this.last = null;
    const previous = this.past.pop();
    if (previous === undefined) return null;
    this.future.push(current);
    return previous;
  }

  /** The recipe one step forward, or null when there is none; `current` becomes undoable. */
  redo(current: T): T | null {
    this.gesture = null;
    this.last = null;
    const next = this.future.pop();
    if (next === undefined) return null;
    this.past.push(current);
    return next;
  }

  /** Forgets everything (another frame was opened). */
  clear(): void {
    this.past = [];
    this.future = [];
    this.gesture = null;
    this.last = null;
  }
}

/** Recipes are plain JSON, so the same text is the same recipe. */
function same(a: unknown, b: unknown): boolean {
  return a === b || JSON.stringify(a) === JSON.stringify(b);
}

/** The paths of the values that differ between two plain JSON values ("tone.contrast"). */
function changedFields(a: unknown, b: unknown, path = '', found = new Set<string>()): Set<string> {
  if (same(a, b)) return found;
  const isObject = (v: unknown): v is Record<string, unknown> =>
    typeof v === 'object' && v !== null;
  if (!isObject(a) || !isObject(b) || Array.isArray(a) !== Array.isArray(b)) {
    found.add(path);
    return found;
  }
  for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) {
    changedFields(a[key], b[key], path ? `${path}.${key}` : key, found);
  }
  return found;
}
