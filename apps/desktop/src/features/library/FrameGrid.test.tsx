import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations, mockLayout } from '../../test-support';
import { ipc, makeFrames, makeRoll, resetFake, state } from './fake-ipc';
import { FrameGrid } from './FrameGrid';
import { resetLibraryStore, useLibraryStore } from './library-store';
import { resetThumbnails } from './thumbnails';

vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);

const fetchPage = (page: number, size: number) =>
  ipc.frameList({ roll_id: null, page, page_size: size });

function renderGrid(emptyText = 'Nothing here') {
  return render(
    <FrameGrid fetchPage={fetchPage} queryKey="all" showRoll={false} emptyText={emptyText} />,
  );
}

const selectedIds = () => [...useLibraryStore.getState().selected].sort();
const frameId = (n: number) => `r-f${String(n).padStart(5, '0')}`;
const cells = () => screen.getAllByRole('gridcell');

describe('FrameGrid', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    // 900 px wide gives 4 columns; 600 px high shows about 3 rows.
    restoreLayout = mockLayout(900, 600);
    state.frames = makeFrames('r', 5000);
    useLibraryStore.setState({ phase: 'ready', rolls: [makeRoll({ id: 'r', name: 'Roll R' })] });
  });
  afterEach(() => {
    restoreLayout();
  });

  it('opens a frame on a double-click and on Enter, with its position', async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    render(
      <FrameGrid
        fetchPage={fetchPage}
        queryKey="all"
        showRoll={false}
        emptyText="Nothing here"
        onOpen={onOpen}
      />,
    );
    await screen.findAllByRole('gridcell');

    await user.dblClick(cells()[2] as HTMLElement);
    expect(onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ id: frameId(3) }), 2);

    // The double-click also focused that cell, so the arrow moves on to the next one.
    screen.getByRole('grid').focus();
    await user.keyboard('{ArrowRight}{Enter}');
    expect(onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ id: frameId(4) }), 3);
    expect(onOpen).toHaveBeenCalledTimes(2);
  });

  it('does nothing on Enter or a double-click when it was not told what to open', async () => {
    const user = userEvent.setup();
    renderGrid();
    await screen.findAllByRole('gridcell');
    await user.dblClick(cells()[1] as HTMLElement);
    screen.getByRole('grid').focus();
    await user.keyboard('{Enter}');
    expect(selectedIds()).toEqual([frameId(2)]);
  });

  it('draws only the rows on screen out of 5,000 frames, and says how big the grid is', async () => {
    renderGrid();
    await screen.findAllByRole('gridcell');
    const grid = screen.getByRole('grid', { name: 'Frames' });
    expect(grid).toHaveAttribute('aria-colcount', '4');
    expect(grid).toHaveAttribute('aria-rowcount', '1250');
    expect(cells().length).toBeGreaterThanOrEqual(8);
    expect(cells().length).toBeLessThanOrEqual(40);
    expect(screen.getByText('Frame 1')).toBeInTheDocument();
    expect(screen.queryByText('Frame 4000')).not.toBeInTheDocument();
    // One page was enough for the first screen.
    expect(ipc.frameList).toHaveBeenCalledTimes(1);
  });

  it('asks for thumbnails of the frames on screen only', async () => {
    renderGrid();
    await waitFor(() => expect(ipc.frameThumbnail).toHaveBeenCalled());
    const asked = ipc.frameThumbnail.mock.calls.map(([id]) => id);
    expect(asked).toContain(frameId(1));
    expect(asked.length).toBeLessThan(60);
    // Decorative images (empty alt) have no role; look for the elements themselves.
    await waitFor(() =>
      expect(document.querySelectorAll('img[src^="blob:"]').length).toBeGreaterThan(0),
    );
  });

  it('shows "No preview" for a frame without a thumbnail and does not ask for it', async () => {
    state.frames = state.frames.map((f) =>
      f.index_in_roll === 2 ? { ...f, has_thumb: false } : f,
    );
    renderGrid();
    const cell = (await screen.findByText('Frame 2')).closest('[role="gridcell"]') as HTMLElement;
    expect(within(cell).getByText('No preview')).toBeInTheDocument();
    expect(ipc.frameThumbnail).not.toHaveBeenCalledWith(frameId(2));
  });

  it('marks edited frames', async () => {
    state.frames = state.frames.map((f) =>
      f.index_in_roll === 1 ? { ...f, edited_at: '2026-10-30T10:00:00Z' } : f,
    );
    renderGrid();
    const cell = (await screen.findByText('Frame 1')).closest('[role="gridcell"]') as HTMLElement;
    expect(within(cell).getByText('Edited')).toBeInTheDocument();
  });

  it('selects with a click, adds with Ctrl, and selects a range with Shift', async () => {
    const user = userEvent.setup();
    renderGrid();
    await screen.findAllByRole('gridcell');

    await user.click(cells()[1] as HTMLElement);
    expect(selectedIds()).toEqual([frameId(2)]);
    expect(cells()[1]).toHaveAttribute('aria-selected', 'true');

    await user.keyboard('{Control>}');
    await user.click(cells()[3] as HTMLElement);
    await user.keyboard('{/Control}');
    expect(selectedIds()).toEqual([frameId(2), frameId(4)]);

    await user.keyboard('{Control>}');
    await user.click(cells()[1] as HTMLElement);
    await user.keyboard('{/Control}');
    expect(selectedIds()).toEqual([frameId(4)]);

    await user.click(cells()[0] as HTMLElement);
    await user.keyboard('{Shift>}');
    await user.click(cells()[5] as HTMLElement);
    await user.keyboard('{/Shift}');
    await waitFor(() => expect(selectedIds()).toEqual([1, 2, 3, 4, 5, 6].map(frameId).sort()));

    await user.click(cells()[2] as HTMLElement);
    expect(selectedIds()).toEqual([frameId(3)]);
  });

  it('moves with the arrow keys and with [ and ], selecting what it lands on', async () => {
    const user = userEvent.setup();
    renderGrid();
    await screen.findAllByRole('gridcell');
    await user.click(cells()[0] as HTMLElement);

    await user.keyboard('{ArrowRight}');
    await waitFor(() => expect(selectedIds()).toEqual([frameId(2)]));
    await user.keyboard('{ArrowDown}');
    await waitFor(() => expect(selectedIds()).toEqual([frameId(6)]));
    await user.keyboard(']');
    await waitFor(() => expect(selectedIds()).toEqual([frameId(7)]));
    await user.keyboard('[[');
    await waitFor(() => expect(selectedIds()).toEqual([frameId(6)]));
    await user.keyboard('{ArrowUp}{ArrowLeft}');
    await waitFor(() => expect(selectedIds()).toEqual([frameId(1)]));

    const grid = screen.getByRole('grid');
    expect(grid).toHaveAttribute('aria-activedescendant', `frame-cell-${frameId(1)}`);
  });

  it('extends the selection with Shift and the arrows, toggles with Space, clears with Escape', async () => {
    const user = userEvent.setup();
    renderGrid();
    await screen.findAllByRole('gridcell');
    await user.click(cells()[0] as HTMLElement);

    await user.keyboard('{Shift>}{ArrowRight}{ArrowRight}{/Shift}');
    await waitFor(() => expect(selectedIds()).toEqual([1, 2, 3].map(frameId)));

    // Ctrl+arrow moves the focus only; Space then adds that frame.
    await user.keyboard('{Control>}{ArrowRight}{/Control}');
    expect(selectedIds()).toEqual([1, 2, 3].map(frameId));
    await user.keyboard(' ');
    expect(selectedIds()).toEqual([1, 2, 3, 4].map(frameId));

    await user.keyboard('{Escape}');
    expect(selectedIds()).toEqual([]);
  });

  it('selects every frame with Ctrl+A, loading pages that were not on screen', async () => {
    const user = userEvent.setup();
    renderGrid();
    await screen.findAllByRole('gridcell');
    await user.click(cells()[0] as HTMLElement);
    await user.keyboard('{Control>}a{/Control}');
    await waitFor(() => expect(useLibraryStore.getState().selected.size).toBe(5000));
    // 5,000 frames are 25 pages of 200.
    expect(ipc.frameList.mock.calls.length).toBe(25);
  });

  it('loads further pages as the grid scrolls', async () => {
    renderGrid();
    await screen.findAllByRole('gridcell');
    const scroller = screen.getByRole('grid').parentElement as HTMLElement;
    // Row 300 is frame 1,201: the seventh page.
    scroller.scrollTop = 300 * 172;
    act(() => {
      scroller.dispatchEvent(new Event('scroll'));
    });
    await waitFor(() => expect(screen.getByText('Frame 1201')).toBeInTheDocument());
    expect(ipc.frameList.mock.calls.some(([q]) => q.page === 6)).toBe(true);
  });

  it('reloads in place when the library changes, without emptying the grid', async () => {
    renderGrid();
    await screen.findByText('Frame 1');
    state.frames = state.frames.slice(0, 3000);
    ipc.frameList.mockClear();
    act(() => useLibraryStore.getState().bump());
    await waitFor(() => expect(ipc.frameList).toHaveBeenCalled());
    expect(screen.getByText('Frame 1')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole('grid')).toHaveAttribute('aria-rowcount', '750'));
  });

  it('shows the empty message when there are no frames', async () => {
    state.frames = [];
    renderGrid('No frames here yet.');
    expect(await screen.findByText('No frames here yet.')).toBeInTheDocument();
    expect(screen.queryByRole('grid')).not.toBeInTheDocument();
  });

  it('lays out in columns when frames arrive in a grid that started empty', async () => {
    state.frames = [];
    renderGrid('No frames here yet.');
    expect(await screen.findByText('No frames here yet.')).toBeInTheDocument();
    state.frames = makeFrames('r', 40);
    act(() => useLibraryStore.getState().bump());
    const grid = await screen.findByRole('grid');
    expect(grid).toHaveAttribute('aria-colcount', '4');
    expect(grid).toHaveAttribute('aria-rowcount', '10');
  });

  it('shows the roll of each frame when asked to', async () => {
    render(<FrameGrid fetchPage={fetchPage} queryKey="all" showRoll emptyText="none" />);
    const cell = (await screen.findByText('Frame 1')).closest('[role="gridcell"]') as HTMLElement;
    expect(within(cell).getByText('Roll R')).toBeInTheDocument();
  });

  it('reports a failure to load instead of an empty grid', async () => {
    ipc.frameList.mockRejectedValueOnce(new Error('database is busy'));
    renderGrid();
    expect(await screen.findByRole('alert')).toHaveTextContent('database is busy');
  });

  it('has no accessibility violations', async () => {
    const { container } = renderGrid();
    await screen.findAllByRole('gridcell');
    await expectNoA11yViolations(container);
  });
});
