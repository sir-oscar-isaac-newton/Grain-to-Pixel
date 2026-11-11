import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import type { FrameFlag } from '../../shared/ipc';
import { expectNoA11yViolations, mockLayout } from '../../test-support';
import { ipc, libraryDetails, makeFrames, makeRoll, resetFake, state } from './fake-ipc';
import { resetLibraryStore, useLibraryStore } from './library-store';
import { LibraryView } from './LibraryView';
import { resetThumbnails } from './thumbnails';

vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);

/** A library with the roll "Summer" (the frames get the flags given, in order), already open. */
function openLibrary(flags: FrameFlag[], over = {}) {
  state.details = libraryDetails(over);
  state.rolls = [makeRoll({ id: 'summer', name: 'Summer' })];
  state.frames = makeFrames('summer', flags.length).map((f, i) => ({
    ...f,
    flag: flags[i] ?? 'none',
  }));
}

const flagOf = (id: string) => state.frames.find((f) => f.id === id)?.flag;
const filter = () => screen.getByRole('combobox', { name: 'Show frames by flag' });
const selectRejected = () => screen.getByRole('button', { name: 'Select rejected' });

describe('LibraryView: pick and reject', () => {
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

  async function show(flags: FrameFlag[], over = {}) {
    openLibrary(flags, over);
    const view = render(<LibraryView />);
    const cells = await screen.findAllByRole('gridcell');
    return { ...view, cells };
  }

  describe('the keys', () => {
    it('P picks, X rejects and U clears the selected frames, and the grid reloads', async () => {
      const user = userEvent.setup();
      const { cells } = await show(['none', 'none', 'none']);
      await user.click(cells[0] as HTMLElement);
      await user.keyboard('{Shift>}{ArrowRight}{/Shift}');
      await waitFor(() => expect(screen.getByText('2 selected')).toBeInTheDocument());

      await user.keyboard('p');
      await waitFor(() =>
        expect(ipc.frameSetFlag).toHaveBeenLastCalledWith(
          ['summer-f00001', 'summer-f00002'],
          'pick',
        ),
      );
      await waitFor(() => expect(screen.getAllByRole('img', { name: 'Picked' })).toHaveLength(2));

      await user.keyboard('X');
      await waitFor(() =>
        expect(ipc.frameSetFlag).toHaveBeenLastCalledWith(
          ['summer-f00001', 'summer-f00002'],
          'reject',
        ),
      );
      await waitFor(() => expect(screen.getAllByRole('img', { name: 'Rejected' })).toHaveLength(2));
      expect(flagOf('summer-f00003')).toBe('none');

      await user.keyboard('u');
      await waitFor(() =>
        expect(screen.queryAllByRole('img', { name: /Picked|Rejected/ })).toHaveLength(0),
      );
      expect(flagOf('summer-f00001')).toBe('none');
      expect(ipc.frameSetFlag).toHaveBeenCalledTimes(3);
    });

    it('ignores the keys with nothing selected', async () => {
      const user = userEvent.setup();
      await show(['none', 'none']);
      screen.getByRole('grid').focus();
      await user.keyboard('pxu');
      expect(ipc.frameSetFlag).not.toHaveBeenCalled();
    });

    it('ignores the keys with a modifier', async () => {
      const user = userEvent.setup();
      const { cells } = await show(['none', 'none']);
      await user.click(cells[0] as HTMLElement);
      await user.keyboard('{Control>}p{/Control}{Alt>}x{/Alt}');
      expect(ipc.frameSetFlag).not.toHaveBeenCalled();
    });

    it('ignores the keys in a read-only library', async () => {
      const user = userEvent.setup();
      const { cells } = await show(['none', 'none'], { read_only: true });
      await user.click(cells[0] as HTMLElement);
      screen.getByRole('grid').focus();
      await user.keyboard('p');
      expect(ipc.frameSetFlag).not.toHaveBeenCalled();
    });

    it('ignores the keys while typing in the search box', async () => {
      const user = userEvent.setup();
      const { cells } = await show(['none', 'none']);
      await user.click(cells[0] as HTMLElement);
      await user.type(screen.getByRole('searchbox', { name: 'Search frames' }), 'pxu');
      expect(ipc.frameSetFlag).not.toHaveBeenCalled();
    });

    it('ignores the keys from a field inside the grid', async () => {
      const user = userEvent.setup();
      const { cells } = await show(['none', 'none']);
      await user.click(cells[0] as HTMLElement);
      const field = document.createElement('input');
      screen.getByRole('grid').appendChild(field);
      fireEvent.keyDown(field, { key: 'p' });
      expect(ipc.frameSetFlag).not.toHaveBeenCalled();
      fireEvent.keyDown(screen.getByRole('grid'), { key: 'p' });
      await waitFor(() => expect(ipc.frameSetFlag).toHaveBeenCalledTimes(1));
    });

    it('says so when the flag cannot be set', async () => {
      const user = userEvent.setup();
      const { cells } = await show(['none', 'none']);
      ipc.frameSetFlag.mockRejectedValueOnce(new Error('disk full'));
      await user.click(cells[0] as HTMLElement);
      await user.keyboard('x');
      expect(
        await screen.findByText('The flag could not be changed: disk full'),
      ).toBeInTheDocument();
    });
  });

  describe('the marks', () => {
    it('show a labelled mark for picked and rejected frames only', async () => {
      const { cells } = await show(['pick', 'reject', 'none']);
      const mark = (cell: HTMLElement | undefined, name: RegExp | string) =>
        within(cell as HTMLElement).queryByRole('img', { name });
      expect(mark(cells[0], 'Picked')).toBeInTheDocument();
      expect(mark(cells[1], 'Rejected')).toBeInTheDocument();
      expect(mark(cells[2], /Picked|Rejected/)).toBeNull();
    });

    it('have no accessibility violations on the grid', async () => {
      await show(['pick', 'reject', 'none']);
      await expectNoA11yViolations(screen.getByRole('grid'));
    });
  });

  describe('the flag filter', () => {
    const cases: [string, FrameFlag][] = [
      ['Picked', 'pick'],
      ['Rejected', 'reject'],
      ['Unflagged', 'none'],
    ];

    it.each(cases)('%s fetches the frames with that flag', async (label, flag) => {
      const user = userEvent.setup();
      await show(['pick', 'reject', 'none', 'none']);
      await user.selectOptions(filter(), label);
      await waitFor(() =>
        expect(ipc.frameList).toHaveBeenLastCalledWith(expect.objectContaining({ flag })),
      );
      await waitFor(() =>
        expect(screen.getAllByRole('gridcell')).toHaveLength(flag === 'none' ? 2 : 1),
      );
    });

    it('All shows every frame again', async () => {
      const user = userEvent.setup();
      await show(['pick', 'reject', 'none']);
      await user.selectOptions(filter(), 'Picked');
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(1));
      await user.selectOptions(filter(), 'All frames');
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(3));
      expect(ipc.frameList).toHaveBeenLastCalledWith(expect.objectContaining({ flag: null }));
    });

    it('applies to the chosen roll', async () => {
      const user = userEvent.setup();
      await show(['pick', 'none']);
      act(() => useLibraryStore.getState().selectRoll('summer'));
      await user.selectOptions(filter(), 'Picked');
      await waitFor(() =>
        expect(ipc.frameList).toHaveBeenLastCalledWith(
          expect.objectContaining({ roll_id: 'summer', flag: 'pick' }),
        ),
      );
    });

    it('is combined with a search', async () => {
      const user = userEvent.setup();
      await show(['pick', 'reject', 'pick']);
      state.frames = state.frames.map((f, i) => (i < 2 ? { ...f, note: 'keeper' } : f));
      await user.selectOptions(filter(), 'Picked');
      await user.type(screen.getByRole('searchbox', { name: 'Search frames' }), 'keeper');
      await waitFor(() =>
        expect(ipc.search).toHaveBeenLastCalledWith(
          expect.objectContaining({ text: 'keeper', flag: 'pick' }),
          0,
          200,
        ),
      );
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(1));
    });

    it('clears the selection', async () => {
      const user = userEvent.setup();
      const { cells } = await show(['pick', 'none']);
      await user.click(cells[0] as HTMLElement);
      await waitFor(() => expect(screen.getByText('1 selected')).toBeInTheDocument());
      await user.selectOptions(filter(), 'Picked');
      expect(useLibraryStore.getState().selected.size).toBe(0);
    });
  });

  describe('Select rejected', () => {
    it('selects exactly the rejected frames, reading every page', async () => {
      const user = userEvent.setup();
      // 1100 frames, every other one rejected: 550 rejected, more than one page of 500.
      const flags = Array.from({ length: 1100 }, (_, i): FrameFlag =>
        i % 2 === 0 ? 'reject' : 'pick',
      );
      await show(flags);
      await user.click(selectRejected());
      await waitFor(() => expect(useLibraryStore.getState().selected.size).toBe(550));
      const ids = [...useLibraryStore.getState().selected];
      expect(ids.every((id) => flagOf(id) === 'reject')).toBe(true);
      const reads = ipc.frameList.mock.calls.filter(
        ([q]) => q.flag === 'reject' && q.page_size === 500,
      );
      expect(reads).toHaveLength(2);
      expect(screen.getByText('550 selected')).toBeInTheDocument();
    });

    it('keeps to the chosen roll', async () => {
      const user = userEvent.setup();
      await show(['reject', 'reject']);
      state.rolls = [...state.rolls, makeRoll({ id: 'winter', name: 'Winter' })];
      state.frames = [
        ...state.frames,
        ...makeFrames('winter', 2).map((f) => ({ ...f, flag: 'reject' as const })),
      ];
      act(() => useLibraryStore.getState().selectRoll('summer'));
      await user.click(selectRejected());
      await waitFor(() => expect(useLibraryStore.getState().selected.size).toBe(2));
      const ids = [...useLibraryStore.getState().selected];
      expect(ids.every((id) => id.startsWith('summer'))).toBe(true);
    });

    it('keeps to a search', async () => {
      const user = userEvent.setup();
      await show(['reject', 'reject', 'pick']);
      state.frames = state.frames.map((f, i) => (i === 0 ? { ...f, note: 'keeper' } : f));
      await user.type(screen.getByRole('searchbox', { name: 'Search frames' }), 'keeper');
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(1));
      await user.click(selectRejected());
      await waitFor(() => expect(useLibraryStore.getState().selected.size).toBe(1));
      expect(ipc.search).toHaveBeenLastCalledWith(
        expect.objectContaining({ text: 'keeper', flag: 'reject' }),
        0,
        500,
      );
    });

    it('can be followed by Delete, and says so when the view has no rejected frame', async () => {
      const user = userEvent.setup();
      await show(['pick', 'reject']);
      await user.click(selectRejected());
      await waitFor(() => expect(screen.getByText('1 selected')).toBeInTheDocument());
      expect(screen.getByRole('button', { name: 'Delete' })).toBeEnabled();
      await user.selectOptions(filter(), 'Picked');
      await user.click(selectRejected());
      expect(await screen.findByText('No frame in this view is rejected.')).toBeInTheDocument();
    });

    it('is off while the frames are being read', async () => {
      const user = userEvent.setup();
      await show(['reject']);
      let release: () => void = () => undefined;
      ipc.frameList.mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            release = () => resolve({ items: [], total: 0, page: 0, page_size: 500 });
          }),
      );
      await user.click(selectRejected());
      expect(selectRejected()).toBeDisabled();
      await act(async () => release());
      await waitFor(() => expect(selectRejected()).toBeEnabled());
    });
  });

  describe('export', () => {
    const optionName = 'Only picked frames';

    async function openExport(flags: FrameFlag[], selectIds: string[]) {
      const user = userEvent.setup();
      await show(flags);
      act(() => useLibraryStore.getState().setSelected(selectIds));
      await user.click(screen.getByRole('button', { name: 'Export…' }));
      const dialog = await screen.findByRole('dialog', { name: 'Export' });
      return { user, dialog };
    }

    it('offers only picked frames when a selected frame is picked, off at first', async () => {
      const { dialog } = await openExport(
        ['pick', 'none', 'reject'],
        ['summer-f00001', 'summer-f00002', 'summer-f00003'],
      );
      const box = await within(dialog).findByRole('checkbox', { name: optionName });
      expect(box).not.toBeChecked();
      expect(within(dialog).getByText(/Write 3 frames/)).toBeInTheDocument();
    });

    it('does not offer it when no selected frame is picked', async () => {
      const { dialog } = await openExport(
        ['pick', 'none', 'reject'],
        ['summer-f00002', 'summer-f00003'],
      );
      await waitFor(() => expect(ipc.frameList).toHaveBeenCalled());
      expect(within(dialog).queryByRole('checkbox', { name: optionName })).toBeNull();
    });

    it('exports only the picked selected frames when ticked, and the count says so', async () => {
      const { user, dialog } = await openExport(
        ['pick', 'none', 'pick', 'pick'],
        ['summer-f00001', 'summer-f00002', 'summer-f00003'],
      );
      await user.click(await within(dialog).findByRole('checkbox', { name: optionName }));
      expect(
        within(dialog).getByText('Write 2 picked frames, with the colour profile embedded.'),
      ).toBeInTheDocument();
      await user.click(within(dialog).getByRole('button', { name: 'Choose folder…' }));
      await within(dialog).findByText('/films/Library');
      await user.click(within(dialog).getByRole('button', { name: 'Export' }));
      await waitFor(() => expect(ipc.exportStart).toHaveBeenCalled());
      expect(state.exports[0]?.frameIds).toEqual(['summer-f00001', 'summer-f00003']);
    });

    it('exports every selected frame when not ticked', async () => {
      const { user, dialog } = await openExport(
        ['pick', 'none'],
        ['summer-f00001', 'summer-f00002'],
      );
      await within(dialog).findByRole('checkbox', { name: optionName });
      await user.click(within(dialog).getByRole('button', { name: 'Choose folder…' }));
      await within(dialog).findByText('/films/Library');
      await user.click(within(dialog).getByRole('button', { name: 'Export' }));
      await waitFor(() => expect(ipc.exportStart).toHaveBeenCalled());
      expect(state.exports[0]?.frameIds).toEqual(['summer-f00001', 'summer-f00002']);
    });

    it('has no accessibility violations with the option shown', async () => {
      const { dialog } = await openExport(['pick', 'none'], ['summer-f00001', 'summer-f00002']);
      await within(dialog).findByRole('checkbox', { name: optionName });
      await expectNoA11yViolations(dialog);
    });
  });
});
