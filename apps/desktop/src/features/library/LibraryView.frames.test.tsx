import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import { expectNoA11yViolations, mockLayout } from '../../test-support';
import { ipc, libraryDetails, makeFrames, makeRoll, resetFake, state } from './fake-ipc';
import { resetLibraryStore } from './library-store';
import { LibraryView } from './LibraryView';
import { resetThumbnails } from './thumbnails';

vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);

/** A library with the roll "Summer" (3 frames, the second with a note), already open. */
function openLibrary(over = {}) {
  state.details = libraryDetails(over);
  state.rolls = [makeRoll({ id: 'summer', name: 'Summer' })];
  state.frames = makeFrames('summer', 3).map((f, i) => (i === 1 ? { ...f, note: 'sharp' } : f));
}

describe('LibraryView: deleting frames and editing notes', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    useUiStore.setState({ view: 'library', developFrame: null });
    restoreLayout = mockLayout(900, 600);
  });
  afterEach(() => restoreLayout());

  async function show(over = {}) {
    openLibrary(over);
    const view = render(<LibraryView />);
    const cells = await screen.findAllByRole('gridcell');
    return { ...view, cells };
  }

  describe('delete', () => {
    it('asks first, then deletes the selected frames and updates the grid and the notice', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      await user.click(cells[0] as HTMLElement);
      await user.keyboard('{Shift>}{ArrowRight}{/Shift}');
      await waitFor(() => expect(screen.getByText('2 selected')).toBeInTheDocument());
      const rollCalls = ipc.rollList.mock.calls.length;

      await user.click(screen.getByRole('button', { name: 'Delete' }));
      const dialog = await screen.findByRole('alertdialog', {
        name: 'Delete the selected frames?',
      });
      expect(within(dialog).getByText(/2 frames will be removed/)).toBeInTheDocument();
      expect(within(dialog).getByText(/cannot be undone/)).toBeInTheDocument();
      expect(within(dialog).getByRole('button', { name: 'Cancel' })).toHaveFocus();

      await user.click(within(dialog).getByRole('button', { name: 'Delete frames' }));
      await waitFor(() =>
        expect(ipc.frameDelete).toHaveBeenCalledWith(['summer-f00001', 'summer-f00002']),
      );
      expect(await screen.findByText('2 frames deleted.')).toBeInTheDocument();
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(1));
      expect(screen.queryByText(/selected/)).not.toBeInTheDocument();
      expect(ipc.rollList.mock.calls.length).toBeGreaterThan(rollCalls);
      expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    });

    it('deletes nothing when cancelled', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      await user.click(cells[0] as HTMLElement);
      await user.click(screen.getByRole('button', { name: 'Delete' }));
      const dialog = await screen.findByRole('alertdialog');
      await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
      expect(ipc.frameDelete).not.toHaveBeenCalled();
      expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
      expect(screen.getAllByRole('gridcell')).toHaveLength(3);
    });

    it('lists the files that could not be removed', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      ipc.frameDelete.mockResolvedValueOnce({ frames_deleted: 1, files_left: ['a/scan.tif'] });
      await user.click(cells[0] as HTMLElement);
      await user.click(screen.getByRole('button', { name: 'Delete' }));
      const dialog = await screen.findByRole('alertdialog');
      await user.click(within(dialog).getByRole('button', { name: 'Delete frames' }));
      expect(
        await screen.findByText(/1 file could not be removed: a\/scan\.tif/),
      ).toBeInTheDocument();
    });

    it('says so when the delete fails', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      ipc.frameDelete.mockRejectedValueOnce(new Error('disk full'));
      await user.click(cells[0] as HTMLElement);
      await user.click(screen.getByRole('button', { name: 'Delete' }));
      const dialog = await screen.findByRole('alertdialog');
      await user.click(within(dialog).getByRole('button', { name: 'Delete frames' }));
      expect(
        await screen.findByText('The frames could not be deleted: disk full'),
      ).toBeInTheDocument();
    });

    it('opens the same dialog with the Delete key in the grid', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      await user.click(cells[0] as HTMLElement);
      screen.getByRole('grid').focus();
      await user.keyboard('{Delete}');
      expect(await screen.findByRole('alertdialog')).toBeInTheDocument();
    });

    it('ignores the Delete key with nothing selected', async () => {
      const user = userEvent.setup();
      await show();
      screen.getByRole('grid').focus();
      await user.keyboard('{Delete}');
      expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    });

    it('is off with nothing selected', async () => {
      await show();
      expect(screen.getByRole('button', { name: 'Delete' })).toBeDisabled();
    });

    it('is off in a read-only library, for the button and the key', async () => {
      const user = userEvent.setup();
      const { cells } = await show({ read_only: true });
      await user.click(cells[0] as HTMLElement);
      expect(screen.getByRole('button', { name: 'Delete' })).toBeDisabled();
      screen.getByRole('grid').focus();
      await user.keyboard('{Delete}');
      expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    });

    it('closes the frame open in Develop when it is deleted, and keeps another one', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      useUiStore.setState({ developFrame: { id: 'summer-f00003', index: 2 } });
      await user.click(cells[0] as HTMLElement);
      await user.click(screen.getByRole('button', { name: 'Delete' }));
      await user.click(
        within(await screen.findByRole('alertdialog')).getByRole('button', {
          name: 'Delete frames',
        }),
      );
      await screen.findByText('1 frame deleted.');
      expect(useUiStore.getState().developFrame).toEqual({ id: 'summer-f00003', index: 2 });

      await user.click(screen.getAllByRole('gridcell')[1] as HTMLElement);
      await user.click(screen.getByRole('button', { name: 'Delete' }));
      await user.click(
        within(await screen.findByRole('alertdialog')).getByRole('button', {
          name: 'Delete frames',
        }),
      );
      await waitFor(() => expect(useUiStore.getState().developFrame).toBeNull());
    });

    it('has no accessibility violations in the dialog', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      await user.click(cells[0] as HTMLElement);
      await user.click(screen.getByRole('button', { name: 'Delete' }));
      await expectNoA11yViolations(await screen.findByRole('alertdialog'));
    });
  });

  describe('the note of a frame', () => {
    const field = () => screen.findByRole('textbox', { name: 'Note for the selected frame' });

    it('shows the note of the one selected frame', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      await user.click(cells[1] as HTMLElement);
      await waitFor(async () => expect(await field()).toHaveValue('sharp'));
    });

    it('saves on blur when changed, and the grid reloads', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      await user.click(cells[1] as HTMLElement);
      const box = await field();
      await waitFor(() => expect(box).toHaveValue('sharp'));
      await user.type(box, ' and warm');
      const loads = ipc.frameList.mock.calls.length;
      await user.tab();
      await waitFor(() =>
        expect(ipc.frameSetNote).toHaveBeenCalledWith('summer-f00002', 'sharp and warm'),
      );
      await waitFor(() => expect(ipc.frameList.mock.calls.length).toBeGreaterThan(loads));
    });

    it('saves on Enter, and Shift+Enter adds a line instead', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      await user.click(cells[0] as HTMLElement);
      const box = await field();
      await waitFor(() => expect(box).toBeEnabled());
      await user.type(box, 'a{Shift>}{Enter}{/Shift}b');
      expect(ipc.frameSetNote).not.toHaveBeenCalled();
      expect(box).toHaveValue('a\nb');
      await user.keyboard('{Enter}');
      await waitFor(() => expect(ipc.frameSetNote).toHaveBeenCalledWith('summer-f00001', 'a\nb'));
    });

    it('does not save an unchanged note', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      await user.click(cells[1] as HTMLElement);
      const box = await field();
      await waitFor(() => expect(box).toHaveValue('sharp'));
      await user.click(box);
      await user.keyboard('{Enter}');
      await user.tab();
      expect(ipc.frameSetNote).not.toHaveBeenCalled();
    });

    it('is read-only in a read-only library', async () => {
      const user = userEvent.setup();
      const { cells } = await show({ read_only: true });
      await user.click(cells[1] as HTMLElement);
      const box = await field();
      await waitFor(() => expect(box).toHaveValue('sharp'));
      expect(box).toHaveAttribute('readonly');
      await user.type(box, 'x{Enter}');
      await user.tab();
      expect(ipc.frameSetNote).not.toHaveBeenCalled();
    });

    it('is hidden with no frame or several frames selected', async () => {
      const user = userEvent.setup();
      const { cells } = await show();
      expect(screen.queryByRole('textbox', { name: 'Note for the selected frame' })).toBeNull();
      await user.click(cells[0] as HTMLElement);
      await field();
      await user.keyboard('{Shift>}{ArrowRight}{/Shift}');
      await waitFor(() =>
        expect(screen.queryByRole('textbox', { name: 'Note for the selected frame' })).toBeNull(),
      );
    });
  });
});
