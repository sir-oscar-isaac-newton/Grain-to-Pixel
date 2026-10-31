import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import { expectNoA11yViolations, mockLayout } from '../../test-support';
import { ipc, libraryDetails, listeners, makeFrames, makeRoll, resetFake, state } from './fake-ipc';
import { resetLibraryStore, useLibraryStore } from './library-store';
import { LibraryView } from './LibraryView';
import { RELOAD_DELAY_MS } from './use-library-events';
import { resetThumbnails } from './thumbnails';

vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);

/** A library with rolls "Summer" (3 frames) and "Winter" (2 frames), already open. */
function openLibrary(over = {}) {
  const summer = makeRoll({
    id: 'summer',
    name: 'Summer',
    film_stock: 'Portra 400',
    camera: 'Nikon F3',
  });
  const winter = makeRoll({ id: 'winter', name: 'Winter', film_stock: 'HP5', camera: 'Leica M6' });
  state.details = libraryDetails(over);
  state.rolls = [summer, winter];
  state.frames = [...makeFrames('summer', 3), ...makeFrames('winter', 2)];
  return { summer, winter };
}

describe('LibraryView', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    restoreLayout = mockLayout(900, 600);
  });
  afterEach(() => {
    restoreLayout();
    vi.useRealTimers();
  });

  describe('opening a library', () => {
    it('shows the rolls and frames of the library that is already open', async () => {
      openLibrary();
      render(<LibraryView />);
      expect(await screen.findByRole('navigation', { name: 'Rolls' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /Summer/ })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /Winter/ })).toBeInTheDocument();
      expect(screen.getByText('Library: Film')).toBeInTheDocument();
      expect(screen.getByText('54.9 GB free')).toBeInTheDocument();
      expect(await screen.findAllByRole('gridcell')).toHaveLength(5);
      expect(ipc.libraryOpen).not.toHaveBeenCalled();
    });

    it('reopens the last library at start', async () => {
      openLibrary();
      const open = state.details;
      state.details = null;
      state.lastLibrary = '/films/Library';
      ipc.libraryOpen.mockImplementationOnce(async () => {
        state.details = open;
        return { app_version: '1', library: open };
      });
      render(<LibraryView />);
      expect(await screen.findByRole('navigation', { name: 'Rolls' })).toBeInTheDocument();
      expect(ipc.libraryOpen).toHaveBeenCalledWith('/films/Library');
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    });

    it('asks which library to use when there is none, and says why the last one failed', async () => {
      state.lastLibrary = '/films/Gone';
      ipc.libraryOpen.mockRejectedValueOnce(new Error('That folder is not a library.'));
      render(<LibraryView />);
      const dialog = await screen.findByRole('dialog', { name: 'Set up your library' });
      expect(within(dialog).getByRole('alert')).toHaveTextContent(
        'Your last library could not be opened: That folder is not a library.',
      );
    });

    it('creates a library: shows the location and free space, then opens it', async () => {
      const user = userEvent.setup();
      render(<LibraryView />);
      const dialog = await screen.findByRole('dialog', { name: 'Set up your library' });
      const create = within(dialog).getByRole('button', { name: 'Create library' });
      expect(create).toBeDisabled();

      await user.click(within(dialog).getByRole('button', { name: 'Choose folder…' }));
      expect(await within(dialog).findByText('54.9 GB')).toBeInTheDocument();
      expect(within(dialog).getAllByText('/films/Library').length).toBeGreaterThan(0);
      expect(within(dialog).getByRole('textbox', { name: 'Library name' })).toHaveValue('Library');

      await user.clear(within(dialog).getByRole('textbox', { name: 'Library name' }));
      await user.type(within(dialog).getByRole('textbox', { name: 'Library name' }), 'Negatives');
      await user.click(create);

      await waitFor(() =>
        expect(ipc.libraryCreate).toHaveBeenCalledWith('/films/Library', 'Negatives'),
      );
      expect(await screen.findByRole('navigation', { name: 'Rolls' })).toBeInTheDocument();
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
      expect(screen.getByText('Library: Negatives')).toBeInTheDocument();
    });

    it('refuses a folder that is not empty and points to the right choice for a library', async () => {
      const user = userEvent.setup();
      state.folder = { exists: true, is_empty: false, has_library: false };
      render(<LibraryView />);
      const dialog = await screen.findByRole('dialog');
      await user.click(within(dialog).getByRole('button', { name: 'Choose folder…' }));
      expect(await within(dialog).findByRole('alert')).toHaveTextContent('not empty');
      expect(within(dialog).getByRole('button', { name: 'Create library' })).toBeDisabled();

      state.folder = { exists: true, is_empty: false, has_library: true };
      await user.click(within(dialog).getByRole('button', { name: 'Choose folder…' }));
      expect(await within(dialog).findByText(/already holds a library/)).toBeInTheDocument();

      await user.click(within(dialog).getByRole('radio', { name: 'Open an existing library' }));
      const open = within(dialog).getByRole('button', { name: 'Open library' });
      await waitFor(() => expect(open).toBeEnabled());
    });

    it('warns when there is little free space, and shows the backend error if creating fails', async () => {
      const user = userEvent.setup();
      ipc.folderCheck.mockResolvedValueOnce({
        path: '/films/Library',
        exists: true,
        is_empty: true,
        has_library: false,
        free_bytes: 5_000_000_000,
      });
      ipc.libraryCreate.mockRejectedValueOnce(new Error('Disk is write-protected.'));
      render(<LibraryView />);
      const dialog = await screen.findByRole('dialog');
      await user.click(within(dialog).getByRole('button', { name: 'Choose folder…' }));
      expect(await within(dialog).findByRole('note')).toHaveTextContent('60 to 100 MB');
      await user.click(within(dialog).getByRole('button', { name: 'Create library' }));
      expect(await within(dialog).findByRole('alert')).toHaveTextContent(
        'Disk is write-protected.',
      );
      expect(within(dialog).getByRole('button', { name: 'Create library' })).toBeEnabled();
    });

    it('cannot be dismissed with Escape', async () => {
      const user = userEvent.setup();
      render(<LibraryView />);
      await screen.findByRole('dialog');
      await user.keyboard('{Escape}');
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    });
  });

  describe('rolls', () => {
    it('shows the frames of the chosen roll, or all of them', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      expect(screen.getByRole('button', { name: 'All frames' })).toHaveAttribute(
        'aria-current',
        'true',
      );

      await user.click(screen.getByRole('button', { name: /Winter/ }));
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(2));
      expect(screen.getByRole('button', { name: /Winter/ })).toHaveAttribute(
        'aria-current',
        'true',
      );
      expect(ipc.frameList).toHaveBeenLastCalledWith({
        roll_id: 'winter',
        page: 0,
        page_size: 200,
      });

      await user.click(screen.getByRole('button', { name: 'All frames' }));
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(5));
    });

    it('creates a roll with its film, camera, ISO and dates, and selects it', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: 'New roll' }));
      const dialog = await screen.findByRole('dialog', { name: 'New roll' });

      await user.type(within(dialog).getByRole('textbox', { name: 'Name' }), '  Autumn walk ');
      await user.type(within(dialog).getByRole('combobox', { name: 'Film' }), 'Ektar 100');
      await user.type(within(dialog).getByRole('combobox', { name: 'Camera' }), 'Nikon F3');
      await user.type(within(dialog).getByRole('textbox', { name: 'Box ISO' }), '100');
      await user.type(within(dialog).getByRole('textbox', { name: 'Shot at ISO' }), '200');
      await user.type(within(dialog).getByRole('textbox', { name: 'Shot from' }), '2026-09');
      await user.type(within(dialog).getByRole('textbox', { name: 'Shot to' }), '2026-09-14');
      await user.type(within(dialog).getByRole('textbox', { name: 'Notes' }), 'pushed one stop');
      await user.click(within(dialog).getByRole('button', { name: 'Save' }));

      await waitFor(() => expect(ipc.rollCreate).toHaveBeenCalledTimes(1));
      expect(ipc.rollCreate).toHaveBeenCalledWith({
        name: 'Autumn walk',
        film_stock: 'Ektar 100',
        camera: 'Nikon F3',
        iso_box: 100,
        iso_shot: 200,
        shot_from: '2026-09',
        shot_to: '2026-09-14',
        notes: 'pushed one stop',
      });
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(screen.getByRole('button', { name: /Autumn walk/ })).toHaveAttribute(
        'aria-current',
        'true',
      );
    });

    it('needs a name, accepts only whole-number ISO, and shows what the backend refuses', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: 'New roll' }));
      const dialog = await screen.findByRole('dialog');

      await user.type(within(dialog).getByRole('textbox', { name: 'Box ISO' }), '4OO');
      await user.click(within(dialog).getByRole('button', { name: 'Save' }));
      expect(within(dialog).getByRole('textbox', { name: 'Name' })).toHaveAccessibleDescription(
        'Give the roll a name.',
      );
      expect(within(dialog).getByRole('textbox', { name: 'Box ISO' })).toHaveAccessibleDescription(
        'ISO must be a whole number above 0.',
      );
      expect(ipc.rollCreate).not.toHaveBeenCalled();

      await user.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'X');
      await user.clear(within(dialog).getByRole('textbox', { name: 'Box ISO' }));
      await user.type(within(dialog).getByRole('textbox', { name: 'Shot from' }), '2026-13');
      ipc.rollCreate.mockRejectedValueOnce(
        new Error('Dates are written as 2026, 2026-07 or 2026-07-14.'),
      );
      await user.click(within(dialog).getByRole('button', { name: 'Save' }));
      expect(await within(dialog).findByRole('alert')).toHaveTextContent('Dates are written as');
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    });

    it('suggests the film and camera of earlier rolls', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: 'New roll' }));
      const dialog = await screen.findByRole('dialog');
      const options = (name: string) => {
        const input = within(dialog).getByRole('combobox', { name });
        const list = document.getElementById(input.getAttribute('list') ?? '');
        return [...(list?.querySelectorAll('option') ?? [])].map((o) => o.getAttribute('value'));
      };
      expect(options('Film')).toEqual(['HP5', 'Portra 400']);
      expect(options('Camera')).toEqual(['Leica M6', 'Nikon F3']);
    });

    it('edits the selected roll, starting from its values', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: /Summer/ }));
      await user.click(screen.getByRole('button', { name: 'Edit roll' }));
      const dialog = await screen.findByRole('dialog', { name: 'Edit roll' });
      expect(within(dialog).getByRole('textbox', { name: 'Name' })).toHaveValue('Summer');
      expect(within(dialog).getByRole('combobox', { name: 'Film' })).toHaveValue('Portra 400');

      await user.clear(within(dialog).getByRole('textbox', { name: 'Name' }));
      await user.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'Summer 2026');
      await user.click(within(dialog).getByRole('button', { name: 'Save' }));
      await waitFor(() => expect(ipc.rollUpdate).toHaveBeenCalledTimes(1));
      expect(ipc.rollUpdate.mock.calls[0]?.[0]).toBe('summer');
      expect(ipc.rollUpdate.mock.calls[0]?.[1]).toMatchObject({
        name: 'Summer 2026',
        film_stock: 'Portra 400',
        camera: 'Nikon F3',
      });
      expect(await screen.findByRole('button', { name: /Summer 2026/ })).toBeInTheDocument();
    });

    it('asks before deleting a roll and says how many frames go with it', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: /Summer/ }));
      await user.click(screen.getByRole('button', { name: 'Delete roll' }));

      const dialog = await screen.findByRole('alertdialog', { name: 'Delete this roll?' });
      expect(await within(dialog).findByText(/“Summer” and its 3 frames/)).toBeInTheDocument();
      // Cancelling changes nothing.
      await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
      expect(ipc.rollDelete).not.toHaveBeenCalled();

      await user.click(screen.getByRole('button', { name: 'Delete roll' }));
      const again = await screen.findByRole('alertdialog');
      await user.click(await within(again).findByRole('button', { name: 'Delete roll' }));
      await waitFor(() => expect(ipc.rollDelete).toHaveBeenCalledWith('summer'));
      expect(await screen.findByText('Roll deleted.')).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /Summer/ })).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'All frames' })).toHaveAttribute(
        'aria-current',
        'true',
      );
    });

    it('names the files that could not be removed after a delete', async () => {
      const user = userEvent.setup();
      openLibrary();
      ipc.rollDelete.mockResolvedValueOnce({ frames_deleted: 3, files_left: ['a/scan.tif'] });
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: /Summer/ }));
      await user.click(screen.getByRole('button', { name: 'Delete roll' }));
      const dialog = await screen.findByRole('alertdialog');
      await user.click(await within(dialog).findByRole('button', { name: 'Delete roll' }));
      expect(
        await screen.findByText(/1 file could not be removed: a\/scan\.tif/),
      ).toBeInTheDocument();
    });
  });

  describe('importing', () => {
    it('imports the picked scans into the roll and follows the job to its result', async () => {
      const user = userEvent.setup();
      openLibrary();
      ipc.pickTiffFiles.mockResolvedValueOnce(['/scans/a.tif', '/scans/b.tif', '/scans/notes.txt']);
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: /Winter/ }));
      await user.click(screen.getByRole('button', { name: 'Import TIFF files…' }));

      await waitFor(() =>
        expect(ipc.importFiles).toHaveBeenCalledWith(
          ['/scans/a.tif', '/scans/b.tif', '/scans/notes.txt'],
          'winter',
          'TiffScan',
        ),
      );
      const jobs = await screen.findByRole('list', { name: 'Background jobs' });
      expect(within(jobs).getByText('Import into Winter')).toBeInTheDocument();

      act(() =>
        listeners.progress.forEach((cb) => cb({ id: 'job-1', stage: 'importing', fraction: 0.5 })),
      );
      expect(within(jobs).getByRole('progressbar', { name: 'Import into Winter' })).toHaveAttribute(
        'value',
        '50',
      );

      act(() =>
        listeners.done.forEach((cb) =>
          cb({
            id: 'job-1',
            result: {
              kind: 'Imported',
              frames: ['x', 'y'],
              frame_ms: [900, 950],
              skipped: [{ path: '/scans/notes.txt', reason: 'NotTiff', details: null }],
            },
          }),
        ),
      );
      expect(within(jobs).getByText('Done')).toBeInTheDocument();
      expect(within(jobs).getByText('2 frames imported, 1 file skipped')).toBeInTheDocument();
      await user.click(within(jobs).getByText('Skipped files'));
      expect(within(jobs).getByText('/scans/notes.txt: not a TIFF file')).toBeInTheDocument();

      await user.click(within(jobs).getByRole('button', { name: 'Dismiss' }));
      expect(screen.queryByRole('list', { name: 'Background jobs' })).not.toBeInTheDocument();
    });

    it('does nothing when the file picker is cancelled', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: /Winter/ }));
      await user.click(screen.getByRole('button', { name: 'Import TIFF files…' }));
      expect(ipc.pickTiffFiles).toHaveBeenCalled();
      expect(ipc.importFiles).not.toHaveBeenCalled();
    });

    it('keeps a job that reported before it was registered, and can cancel a running one', async () => {
      const user = userEvent.setup();
      openLibrary();
      ipc.pickTiffFiles.mockResolvedValueOnce(['/scans/a.tif']);
      ipc.importFiles.mockImplementationOnce(async () => {
        // The job finishes before the command has even returned its id.
        listeners.done.forEach((cb) =>
          cb({
            id: 'fast',
            result: { kind: 'Imported', frames: ['x'], frame_ms: [10], skipped: [] },
          }),
        );
        return 'fast';
      });
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: /Winter/ }));
      await user.click(screen.getByRole('button', { name: 'Import TIFF files…' }));
      const jobs = await screen.findByRole('list', { name: 'Background jobs' });
      expect(within(jobs).getByText('1 frame imported')).toBeInTheDocument();

      act(() => useLibraryStore.getState().dismissJob('fast'));
      act(() =>
        useLibraryStore.getState().addJob({
          id: 'slow',
          kind: 'import',
          rollName: 'Winter',
          fraction: 0.2,
          state: 'running',
        }),
      );
      await user.click(await screen.findByRole('button', { name: 'Cancel' }));
      expect(ipc.jobCancel).toHaveBeenCalledWith('slow');
      act(() =>
        listeners.error.forEach((cb) =>
          cb({ id: 'slow', error: { code: 'Cancelled', message: 'The job was cancelled.' } }),
        ),
      );
      expect(await screen.findByText('Cancelled')).toBeInTheDocument();
    });

    it('shows a failed job with the backend message, and a start failure as a notice', async () => {
      const user = userEvent.setup();
      openLibrary();
      ipc.pickTiffFiles.mockResolvedValue(['/scans/a.tif']);
      ipc.importFiles.mockRejectedValueOnce(new Error('The roll does not exist.'));
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: /Winter/ }));
      await user.click(screen.getByRole('button', { name: 'Import TIFF files…' }));
      expect(
        await screen.findByText('The import could not start: The roll does not exist.'),
      ).toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Dismiss' }));
      expect(screen.queryByText(/could not start/)).not.toBeInTheDocument();

      act(() =>
        useLibraryStore.getState().addJob({
          id: 'bad',
          kind: 'import',
          rollName: 'Winter',
          fraction: 0.3,
          state: 'running',
        }),
      );
      act(() =>
        listeners.error.forEach((cb) =>
          cb({ id: 'bad', error: { code: 'Io', message: 'The disk is full.' } }),
        ),
      );
      expect(await screen.findByText('Failed')).toBeInTheDocument();
      expect(screen.getByText('The disk is full.')).toBeInTheDocument();
    });
  });

  describe('opening a frame in Develop', () => {
    beforeEach(() => useUiStore.setState({ view: 'library', developFrame: null }));

    it('opens the frame that is double-clicked, with its place in the list', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<LibraryView />);
      const cells = await screen.findAllByRole('gridcell');
      await user.dblClick(cells[2] as HTMLElement);
      expect(useUiStore.getState().view).toBe('develop');
      expect(useUiStore.getState().developFrame).toEqual({ id: 'summer-f00003', index: 2 });
    });

    it('opens the focused frame with Enter', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      screen.getByRole('grid').focus();
      await user.keyboard('{ArrowRight}{Enter}');
      expect(useUiStore.getState().developFrame).toEqual({ id: 'summer-f00002', index: 1 });
      expect(useUiStore.getState().view).toBe('develop');
    });

    it('keeps the library, the roll and the search when you come back from Develop', async () => {
      const user = userEvent.setup();
      openLibrary();
      const { unmount } = render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: /Winter/ }));
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(2));
      unmount();
      ipc.libraryInfo.mockClear();

      // Coming back: the same library, still open, with the roll that was chosen.
      render(<LibraryView />);
      expect(await screen.findAllByRole('gridcell')).toHaveLength(2);
      expect(ipc.libraryInfo).not.toHaveBeenCalled();
      expect(screen.getByRole('button', { name: /Winter/ })).toHaveAttribute(
        'aria-current',
        'true',
      );
    });
  });

  describe('live updates', () => {
    it('reloads the frames and rolls once after a burst of library changes', async () => {
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      ipc.frameList.mockClear();
      ipc.rollList.mockClear();
      vi.useFakeTimers({ shouldAdvanceTime: true });

      state.frames = [...state.frames, ...makeFrames('winter', 3, 'late')].map((f, i) => ({
        ...f,
        index_in_roll: f.index_in_roll,
        id: f.id + (i > 4 ? '-new' : ''),
      }));
      act(() => {
        for (let i = 0; i < 5; i += 1) listeners.changed.forEach((cb) => cb('winter', []));
      });
      expect(ipc.frameList).not.toHaveBeenCalled();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(RELOAD_DELAY_MS + 10);
      });
      await waitFor(() => expect(ipc.frameList).toHaveBeenCalledTimes(1));
      expect(ipc.rollList).toHaveBeenCalledTimes(1);
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(8));
    });

    it('loads the pictures again for frames whose thumbnails were regenerated, and only those', async () => {
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      const picture = (n: number) => {
        const cell = [...document.querySelectorAll('[role="gridcell"]')][n];
        return cell?.querySelector('img')?.getAttribute('src');
      };
      await waitFor(() => expect(picture(0)).toMatch(/^blob:/));
      await waitFor(() => expect(picture(1)).toMatch(/^blob:/));
      const first = picture(0);
      const second = picture(1);
      ipc.frameThumbnail.mockClear();

      act(() => {
        listeners.changed.forEach((cb) => cb(null, ['summer-f00001']));
      });
      await waitFor(() => expect(ipc.frameThumbnail).toHaveBeenCalledWith('summer-f00001'));
      await waitFor(() => expect(picture(0)).not.toBe(first));
      expect(picture(0)).toMatch(/^blob:/);
      expect(picture(1)).toBe(second);
      // The quiet-period reload of the grid follows, and asks for no other picture.
      await new Promise((resolve) => setTimeout(resolve, RELOAD_DELAY_MS + 100));
      expect(ipc.frameThumbnail).toHaveBeenCalledTimes(1);
    });

    it('stops listening when it is closed', async () => {
      openLibrary();
      const { unmount } = render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      expect(listeners.changed).toHaveLength(1);
      unmount();
      await waitFor(() => expect(listeners.changed).toHaveLength(0));
      expect(listeners.progress).toHaveLength(0);
    });
  });

  describe('a library from a newer version', () => {
    it('can be browsed, says so, and cannot be changed', async () => {
      openLibrary({ read_only: true });
      render(<LibraryView />);
      expect(await screen.findByText(/made by a newer version/)).toBeInTheDocument();
      expect(await screen.findAllByRole('gridcell')).toHaveLength(5);
      expect(screen.getByRole('button', { name: 'New roll' })).toBeDisabled();
      const user = userEvent.setup();
      await user.click(screen.getByRole('button', { name: /Summer/ }));
      for (const name of ['Edit roll', 'Delete roll', 'Import TIFF files…']) {
        expect(screen.getByRole('button', { name })).toBeDisabled();
      }
    });
  });

  describe('accessibility', () => {
    it('has no violations with a library open and a roll chosen', async () => {
      const user = userEvent.setup();
      openLibrary();
      const { container } = render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: /Summer/ }));
      await waitFor(() => expect(screen.getAllByRole('gridcell')).toHaveLength(3));
      await expectNoA11yViolations(container);
    });

    it('has no violations in the first-run dialog, the roll editor or the delete confirmation', async () => {
      const user = userEvent.setup();
      const first = render(<LibraryView />);
      const firstRun = await screen.findByRole('dialog');
      await expectNoA11yViolations(firstRun);
      first.unmount();

      resetLibraryStore();
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      await user.click(screen.getByRole('button', { name: 'New roll' }));
      await expectNoA11yViolations(await screen.findByRole('dialog'));
      await user.click(screen.getByRole('button', { name: 'Cancel' }));
      await user.click(screen.getByRole('button', { name: /Summer/ }));
      await user.click(screen.getByRole('button', { name: 'Delete roll' }));
      await expectNoA11yViolations(await screen.findByRole('alertdialog'));
    });
  });
});
