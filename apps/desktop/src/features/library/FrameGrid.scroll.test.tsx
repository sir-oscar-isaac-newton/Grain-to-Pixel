// Scrolling a 5,000-frame grid (U-01: "scrolls smoothly, measured fps reported").
//
// jsdom does no layout or painting, so this measures what the grid itself costs per scroll step:
// the virtualizer's work and React rendering the rows that come into view. A browser adds layout
// and paint on top, but it is the part this code controls, and it is repeatable in CI. The result
// is printed so the pull request can quote it.
import { act, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mockLayout } from '../../test-support';
import { ipc, makeFrames, makeRoll, resetFake, state } from './fake-ipc';
import { FrameGrid } from './FrameGrid';
import { ROW_HEIGHT } from './grid-navigation';
import { resetLibraryStore, useLibraryStore } from './library-store';
import { resetThumbnails } from './thumbnails';

vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);

const FRAMES = 5000;
const ROWS = FRAMES / 4;
const STEPS = 250;
/**
 * A step must cost well under this, or scrolling could not hold 20 frames a second. It is set well
 * above what the grid costs (about 10 ms here) so a slow CI runner does not fail it; the printed
 * figure is the measurement.
 */
const BUDGET_MS = 1000 / 20;

describe('FrameGrid scrolling', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    restoreLayout = mockLayout(900, 600); // 4 columns, about 3 rows on screen
    state.frames = makeFrames('r', FRAMES);
    useLibraryStore.setState({ phase: 'ready', rolls: [makeRoll({ id: 'r', name: 'Roll R' })] });
  });
  afterEach(() => restoreLayout());

  it('draws a step of a 5,000-frame scroll well inside a 20 fps frame budget', async () => {
    render(
      <FrameGrid
        fetchPage={(page, size) => ipc.frameList({ roll_id: null, page, page_size: size })}
        queryKey="all"
        showRoll={false}
        emptyText="Nothing here"
      />,
    );
    await screen.findAllByRole('gridcell');
    const scroller = screen.getByRole('grid').parentElement as HTMLElement;

    const step = (top: number) =>
      act(() => {
        scroller.scrollTop = top;
        scroller.dispatchEvent(new Event('scroll'));
      });

    const taken: number[] = [];
    let mostCells = 0;
    for (let i = 1; i <= STEPS; i++) {
      // From the top to the bottom in even steps, a few rows at a time.
      const top = Math.round((i / STEPS) * (ROWS - 3) * ROW_HEIGHT);
      const start = performance.now();
      await step(top);
      taken.push(performance.now() - start);
      // A plain selector: role queries are slow and would make the test itself the bottleneck.
      mostCells = Math.max(mostCells, scroller.querySelectorAll('[role="gridcell"]').length);
    }

    const sorted = [...taken].sort((a, b) => a - b);
    const mean = taken.reduce((a, b) => a + b, 0) / taken.length;
    const p95 = sorted[Math.floor(sorted.length * 0.95)] ?? 0;
    const worst = sorted[sorted.length - 1] ?? 0;
    // eslint-disable-next-line no-console -- the measurement is the point of this test
    console.info(
      `scroll of ${FRAMES} frames, ${STEPS} steps: mean ${mean.toFixed(1)} ms, p95 ${p95.toFixed(1)} ms, ` +
        `worst ${worst.toFixed(1)} ms, about ${Math.round(1000 / mean)} steps per second; ` +
        `at most ${mostCells} cells drawn`,
    );

    // About 4 rows on screen plus 3 rows of overscan each way, at 4 columns.
    expect(mostCells).toBeLessThanOrEqual(60);
    expect(mean).toBeLessThan(BUDGET_MS);
    expect(p95).toBeLessThan(BUDGET_MS * 2);
    // The bottom of the grid is reachable: its pages load as it gets there.
    await waitFor(() => expect(screen.getByText(`Frame ${FRAMES}`)).toBeInTheDocument());
    // Pages arrive on demand, never all at once (5,000 frames are 25 pages).
    expect(ipc.frameList.mock.calls.length).toBeLessThanOrEqual(25);
    // Slow runners get room: the default 5 s is too tight for 250 steps plus the checks.
  }, 60_000);
});
