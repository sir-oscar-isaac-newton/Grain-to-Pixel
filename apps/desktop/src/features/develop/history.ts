/** How many steps an {@link EditHistory} keeps; the oldest is dropped beyond it. */
export const HISTORY_LIMIT = 50;

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

  constructor(private readonly limit: number = HISTORY_LIMIT) {}

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
   * recorded.
   */
  record(before: T, after: T, dragging: boolean): boolean {
    if (dragging) {
      this.gesture ??= { before };
      return false;
    }
    const start = this.gesture ? this.gesture.before : before;
    this.gesture = null;
    if (same(start, after)) return false;
    this.past.push(start);
    if (this.past.length > this.limit) this.past.shift();
    this.future = [];
    return true;
  }

  /** The recipe one step back, or null when there is none; `current` becomes redoable. */
  undo(current: T): T | null {
    this.gesture = null;
    const previous = this.past.pop();
    if (previous === undefined) return null;
    this.future.push(current);
    return previous;
  }

  /** The recipe one step forward, or null when there is none; `current` becomes undoable. */
  redo(current: T): T | null {
    this.gesture = null;
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
  }
}

/** Recipes are plain JSON, so the same text is the same recipe. */
function same(a: unknown, b: unknown): boolean {
  return a === b || JSON.stringify(a) === JSON.stringify(b);
}
