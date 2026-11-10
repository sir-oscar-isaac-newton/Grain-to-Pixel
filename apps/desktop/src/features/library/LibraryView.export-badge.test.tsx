import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import { expectNoA11yViolations, mockLayout } from '../../test-support';
import { ipc, libraryDetails, listeners, makeFrames, makeRoll, resetFake, state } from './fake-ipc';
import { resetLibraryStore, useLibraryStore } from './library-store';
import { LibraryView } from './LibraryView';
import { resetThumbnails } from './thumbnails';

vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);

const EXPORTED = '2026-11-02T10:00:00Z';
const EDITED_AFTER = '2026-11-03T10:00:00Z';
const EDITED_BEFORE = '2026-11-01T10:00:00Z';

/**
 * Four frames of "Summer": 1 never exported, 2 exported and unchanged, 3 exported then edited,
 * 4 edited but never exported.
 */
function openLibrary() {
  state.details = libraryDetails();
  state.rolls = [makeRoll({ id: 'summer', name: 'Summer' })];
  const frames = makeFrames('summer', 4);
  state.frames = frames.map((f, i) => {
    if (i === 1) return { ...f, exported_at: EXPORTED, edited_at: EDITED_BEFORE };
    if (i === 2) return { ...f, exported_at: EXPORTED, edited_at: EDITED_AFTER, note: 'keeper' };
    if (i === 3) return { ...f, edited_at: EDITED_AFTER, note: 'keeper' };
    return f;
  });
}

const needsExport = () => screen.getByRole('checkbox', { name: 'Needs export' });
const flagFilter = () => screen.getByRole('combobox', { name: 'Show frames by flag' });

describe('LibraryView: export state', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    localStorage.clear();
    useUiStore.setState({ view: 'library', developFrame: null });
    restoreLayout = mockLayout(900, 600);
  });
  afterEach(() => restoreLayout());

  async function show() {
    openLibrary();
    const view = render(<LibraryView />);
    const cells = await screen.findAllByRole('gridcell');
    return { ...view, cells };
  }

  describe('the marks', () => {
    it('label frames edited since their export and frames exported and unchanged', async () => {
      const { cells } = await show();
      expect(screen.getAllByRole('img', { name: 'Edited since last export' })).toHaveLength(1);
      expect(screen.getAllByRole('img', { name: 'Exported' })).toHaveLength(1);
      expect(
        within(cells[2] as HTMLElement).getByRole('img', { name: 'Edited since last export' }),
      ).toBeInTheDocument();
      expect(
        within(cells[1] as HTMLElement).getByRole('img', { name: 'Exported' }),
      ).toBeInTheDocument();
    });

    it('show nothing on a frame never exported, edited or not', async () => {
      const { cells } = await show();
      for (const cell of [cells[0], cells[3]] as HTMLElement[]) {
        expect(
          within(cell).queryByRole('img', { name: /Exported|Edited since last export/ }),
        ).toBeNull();
      }
    });

    it('have no accessibility violations on the grid', async () => {
      const { container } = await show();
      await expectNoA11yViolations(container);
    });
  });

  describe('the Needs export filter', () => {
    it('is off at first and fetches only the frames that need an export when ticked', async () => {
      const user = userEvent.setup();
      await show();
      expect(needsExport()).not.toBeChecked();
      expect(ipc.frameList).toHaveBeenLastCalledWith(
        expect.not.objectContaining({ needs_export: true }),
      );
      await user.click(needsExport());
      await waitFor(() =>
        expect(ipc.frameList).toHaveBeenLastCalledWith(
          expect.objectContaining({ needs_export: true }),
        ),
      );
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(1));
      expect(screen.getByRole('img', { name: 'Edited since last export' })).toBeInTheDocument();
      await user.click(needsExport());
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(4));
    });

    it('is combined with a flag', async () => {
      const user = userEvent.setup();
      await show();
      state.frames = state.frames.map((f) =>
        f.id === 'summer-f00003' ? { ...f, flag: 'pick' } : f,
      );
      await user.selectOptions(flagFilter(), 'Picked');
      await user.click(needsExport());
      await waitFor(() =>
        expect(ipc.frameList).toHaveBeenLastCalledWith(
          expect.objectContaining({ flag: 'pick', needs_export: true }),
        ),
      );
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(1));
      await user.selectOptions(flagFilter(), 'Rejected');
      await waitFor(() => expect(screen.queryAllByRole('gridcell')).toHaveLength(0));
    });

    it('is combined with a search', async () => {
      const user = userEvent.setup();
      await show();
      await user.click(needsExport());
      await user.type(screen.getByRole('searchbox', { name: 'Search frames' }), 'keeper');
      await waitFor(() =>
        expect(ipc.search).toHaveBeenLastCalledWith(
          expect.objectContaining({ text: 'keeper', needs_export: true }),
          0,
          200,
        ),
      );
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(1));
      expect(screen.getByRole('img', { name: 'Edited since last export' })).toBeInTheDocument();
    });

    it('clears the selection', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      await user.click(cells[0] as HTMLElement);
      await waitFor(() => expect(screen.getByText('1 selected')).toBeInTheDocument());
      await user.click(needsExport());
      expect(useLibraryStore.getState().selected.size).toBe(0);
    });

    it('is reset when a library is entered', async () => {
      const user = userEvent.setup();
      await show();
      await user.click(needsExport());
      expect(useLibraryStore.getState().needsExport).toBe(true);
      await act(() => useLibraryStore.getState().openLibrary('/films/Other'));
      expect(useLibraryStore.getState().needsExport).toBe(false);
    });

    it('is respected by Select rejected', async () => {
      const user = userEvent.setup();
      await show();
      state.frames = state.frames.map((f) =>
        f.id === 'summer-f00002' || f.id === 'summer-f00003' ? { ...f, flag: 'reject' } : f,
      );
      await user.click(needsExport());
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(1));
      await user.click(screen.getByRole('button', { name: 'Select rejected' }));
      await waitFor(() =>
        expect(ipc.frameList).toHaveBeenLastCalledWith(
          expect.objectContaining({ flag: 'reject', needs_export: true }),
        ),
      );
      await waitFor(() =>
        expect([...useLibraryStore.getState().selected]).toEqual(['summer-f00003']),
      );
    });

    it('has no accessibility violations', async () => {
      const user = userEvent.setup();
      const { container } = await show();
      await user.click(needsExport());
      await expectNoA11yViolations(container);
    });
  });

  describe('after an export', () => {
    it('reloads the grid when the export job finishes, so the marks update', async () => {
      const user = userEvent.setup();
      await show();
      // Frame 3 was edited after its export; frame 1 was never exported.
      act(() => useLibraryStore.getState().setSelected(['summer-f00001', 'summer-f00003']));
      await user.click(screen.getByRole('button', { name: 'Export…' }));
      const dialog = await screen.findByRole('dialog', { name: 'Export' });
      await user.click(within(dialog).getByRole('button', { name: 'Choose folder…' }));
      await within(dialog).findByText('/films/Library');
      await user.click(within(dialog).getByRole('button', { name: 'Export' }));
      await waitFor(() => expect(ipc.exportStart).toHaveBeenCalled());
      // The dialog hides the grid from the accessibility tree, hence `hidden`.
      // The fake logged the export, but the grid shows what it loaded before the job is done.
      expect(
        screen.getAllByRole('img', { name: 'Edited since last export', hidden: true }),
      ).toHaveLength(1);
      const loads = ipc.frameList.mock.calls.length;

      act(() =>
        listeners.done.forEach((cb) =>
          cb({
            id: 'job-export-1',
            result: { kind: 'Exported', frame_ms: [], files: [], failed: [] } as never,
          }),
        ),
      );
      await waitFor(() => expect(ipc.frameList.mock.calls.length).toBeGreaterThan(loads));
      await waitFor(() =>
        expect(
          screen.queryAllByRole('img', { name: 'Edited since last export', hidden: true }),
        ).toHaveLength(0),
      );
      expect(screen.getAllByRole('img', { name: 'Exported', hidden: true })).toHaveLength(3);
    });
  });
});
