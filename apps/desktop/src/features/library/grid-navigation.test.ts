import { describe, expect, it } from 'vitest';
import { CELL_GAP, CELL_WIDTH, columnsFor, rangeBetween, targetIndex } from './grid-navigation';

const opts = { columns: 4, total: 10, ctrl: false, pageRows: 2 };

describe('columnsFor', () => {
  it('fits as many cells as the width allows, and at least one', () => {
    expect(columnsFor(0)).toBe(1);
    expect(columnsFor(100)).toBe(1);
    expect(columnsFor(CELL_WIDTH)).toBe(1);
    expect(columnsFor(2 * CELL_WIDTH + CELL_GAP)).toBe(2);
    expect(columnsFor(2 * CELL_WIDTH + CELL_GAP - 1)).toBe(1);
    expect(columnsFor(1280)).toBe(7);
    expect(columnsFor(1000)).toBe(5);
  });
});

describe('targetIndex', () => {
  it('moves by one frame with the left and right arrows and with [ and ]', () => {
    expect(targetIndex('ArrowRight', 3, opts)).toBe(4);
    expect(targetIndex(']', 3, opts)).toBe(4);
    expect(targetIndex('ArrowLeft', 3, opts)).toBe(2);
    expect(targetIndex('[', 3, opts)).toBe(2);
  });

  it('stops at the first and last frame', () => {
    expect(targetIndex('ArrowLeft', 0, opts)).toBe(0);
    expect(targetIndex('[', 0, opts)).toBe(0);
    expect(targetIndex('ArrowRight', 9, opts)).toBe(9);
    expect(targetIndex(']', 9, opts)).toBe(9);
  });

  it('moves by a row with the up and down arrows, and stays put at the edges', () => {
    expect(targetIndex('ArrowDown', 1, opts)).toBe(5);
    expect(targetIndex('ArrowUp', 5, opts)).toBe(1);
    expect(targetIndex('ArrowUp', 2, opts)).toBe(2);
    // Index 7 has no frame below it (11 does not exist).
    expect(targetIndex('ArrowDown', 7, opts)).toBe(7);
  });

  it('goes to the ends of the row, or of the grid with Ctrl', () => {
    expect(targetIndex('Home', 6, opts)).toBe(4);
    expect(targetIndex('End', 5, opts)).toBe(7);
    expect(targetIndex('End', 9, opts)).toBe(9);
    expect(targetIndex('End', 8, opts)).toBe(9);
    expect(targetIndex('Home', 6, { ...opts, ctrl: true })).toBe(0);
    expect(targetIndex('End', 1, { ...opts, ctrl: true })).toBe(9);
  });

  it('moves a screen with the page keys', () => {
    expect(targetIndex('PageDown', 0, opts)).toBe(8);
    expect(targetIndex('PageDown', 5, opts)).toBe(9);
    expect(targetIndex('PageUp', 9, opts)).toBe(1);
    expect(targetIndex('PageUp', 3, opts)).toBe(0);
  });

  it('ignores other keys and an empty grid', () => {
    expect(targetIndex('a', 3, opts)).toBeNull();
    expect(targetIndex('Enter', 3, opts)).toBeNull();
    expect(targetIndex('ArrowRight', 0, { ...opts, total: 0 })).toBeNull();
  });
});

describe('rangeBetween', () => {
  it('orders the two ends', () => {
    expect(rangeBetween(2, 7)).toEqual([2, 7]);
    expect(rangeBetween(7, 2)).toEqual([2, 7]);
    expect(rangeBetween(3, 3)).toEqual([3, 3]);
  });
});
