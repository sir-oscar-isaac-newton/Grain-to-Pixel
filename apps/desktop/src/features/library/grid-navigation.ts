// Layout and keyboard rules of the frame grid, kept free of React so they can be tested directly.

/** Width of a grid cell in pixels, and the gap between cells. */
export const CELL_WIDTH = 176;
export const CELL_GAP = 8;
/** Height of one grid row, gap included (a thumbnail area of 120 px plus a two-line caption). */
export const ROW_HEIGHT = 172;

/** How many cells fit side by side in `width` pixels (at least one). */
export function columnsFor(width: number): number {
  return Math.max(1, Math.floor((width + CELL_GAP) / (CELL_WIDTH + CELL_GAP)));
}

interface MoveOptions {
  columns: number;
  total: number;
  /** Ctrl (or Cmd) is held: Home and End go to the first and last frame. */
  ctrl: boolean;
  /** Rows a page key moves by. */
  pageRows: number;
}

const clamp = (index: number, total: number) => Math.min(Math.max(0, index), total - 1);

/**
 * Where the focus goes when `key` is pressed on `index`, or null when the key is not a navigation
 * key. Arrows move by one frame or one row; `[` and `]` go to the previous and next frame; Home and
 * End go to the start and end of the row (of the whole grid with Ctrl); Page keys move a screen.
 */
export function targetIndex(key: string, index: number, o: MoveOptions): number | null {
  if (o.total <= 0) return null;
  const rowStart = index - (index % o.columns);
  switch (key) {
    case 'ArrowLeft':
    case '[':
      return clamp(index - 1, o.total);
    case 'ArrowRight':
    case ']':
      return clamp(index + 1, o.total);
    case 'ArrowUp':
      return index - o.columns < 0 ? index : index - o.columns;
    case 'ArrowDown':
      return index + o.columns > o.total - 1 ? index : index + o.columns;
    case 'Home':
      return o.ctrl ? 0 : rowStart;
    case 'End':
      return o.ctrl ? o.total - 1 : clamp(rowStart + o.columns - 1, o.total);
    case 'PageUp':
      return clamp(index - o.columns * o.pageRows, o.total);
    case 'PageDown':
      return clamp(index + o.columns * o.pageRows, o.total);
    default:
      return null;
  }
}

/** The positions from `a` to `b` in order, whichever is larger. */
export function rangeBetween(a: number, b: number): [number, number] {
  return a <= b ? [a, b] : [b, a];
}
