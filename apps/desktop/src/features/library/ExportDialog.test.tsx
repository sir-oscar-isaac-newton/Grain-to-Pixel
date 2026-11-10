import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations, mockLayout } from '../../test-support';
import { exampleName } from './ExportDialog';
import { ipc, libraryDetails, listeners, makeFrames, makeRoll, resetFake, state } from './fake-ipc';
import { resetLibraryStore, useLibraryStore } from './library-store';
import { LibraryView } from './LibraryView';
import { resetThumbnails } from './thumbnails';

vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);

function openLibrary() {
  state.details = libraryDetails();
  state.rolls = [makeRoll({ id: 'summer', name: 'Summer' })];
  state.frames = makeFrames('summer', 3);
}

const exportButton = () => screen.getByRole('button', { name: 'Export…' });
const dialog = () => screen.getByRole('dialog', { name: 'Export' });

/** Opens the library, selects two frames and opens the export dialog. */
async function openDialog() {
  const user = userEvent.setup();
  openLibrary();
  render(<LibraryView />);
  await screen.findAllByRole('gridcell');
  act(() => useLibraryStore.getState().setSelected(['summer-f00001', 'summer-f00002']));
  await user.click(exportButton());
  await screen.findByRole('dialog', { name: 'Export' });
  return user;
}

/** Chooses the folder and starts the export; the job is running afterwards. */
async function start(user: ReturnType<typeof userEvent.setup>) {
  await user.click(within(dialog()).getByRole('button', { name: 'Choose folder…' }));
  await within(dialog()).findByText('/films/Library');
  await user.click(within(dialog()).getByRole('button', { name: 'Export' }));
  await within(dialog()).findByRole('progressbar', { name: 'Export 2 frames' });
}

const finish = (result: Record<string, unknown>) =>
  act(() =>
    listeners.done.forEach((cb) =>
      cb({ id: 'job-export-1', result: { kind: 'Exported', frame_ms: [], ...result } as never }),
    ),
  );

describe('ExportDialog', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    restoreLayout = mockLayout(900, 600);
  });
  afterEach(() => {
    restoreLayout();
    vi.restoreAllMocks();
  });

  describe('opening it', () => {
    it('is out of reach until frames are selected', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      expect(exportButton()).toBeDisabled();
      act(() => useLibraryStore.getState().setSelected(['summer-f00001']));
      expect(exportButton()).toBeEnabled();
      await user.click(exportButton());
      expect(
        within(await screen.findByRole('dialog', { name: 'Export' })).getByText(
          'Write 1 frame at full size, with the colour profile embedded.',
        ),
      ).toBeInTheDocument();
    });

    it('starts with TIFF 16-bit and sRGB, and needs a folder before it can start', async () => {
      await openDialog();
      const d = within(dialog());
      expect(
        d.getByText('Write 2 frames at full size, with the colour profile embedded.'),
      ).toBeInTheDocument();
      expect(d.getByRole('radio', { name: /TIFF, 16-bit/ })).toBeChecked();
      expect(d.getByRole('radio', { name: /JPEG/ })).not.toBeChecked();
      expect(d.getByRole('combobox', { name: /Colour space/ })).toHaveValue('srgb');
      expect(d.queryByRole('slider')).not.toBeInTheDocument();
      expect(d.getByText('No folder chosen')).toBeInTheDocument();
      expect(d.getByRole('button', { name: 'Export' })).toBeDisabled();
      expect(d.getByRole('textbox', { name: /File name/ })).toHaveValue('{roll}_{index:03}');
      expect(d.getByText('Example: Roll_001.tif')).toBeInTheDocument();
      expect(d.getByRole('checkbox', { name: 'Write metadata' })).toBeChecked();
      expect(d.getByRole('checkbox', { name: 'Write metadata' })).toHaveAccessibleDescription(
        /film, camera, ISO, first shooting date and notes you entered/,
      );
    });

    it('shows the quality for JPEG, and says what each colour space is for', async () => {
      const user = await openDialog();
      const d = within(dialog());
      await user.click(d.getByRole('radio', { name: /JPEG/ }));
      expect(d.getByRole('slider', { name: 'JPEG quality' })).toHaveAttribute(
        'aria-valuenow',
        '92',
      );
      expect(d.getByText('Example: Roll_001.jpg')).toBeInTheDocument();

      expect(d.getByText('The safe choice for screens and sharing.')).toBeInTheDocument();
      await user.selectOptions(d.getByRole('combobox', { name: /Colour space/ }), 'prophoto');
      expect(
        d.getByText('The widest range, for further editing in other software.'),
      ).toBeInTheDocument();
    });
  });

  describe('the file name', () => {
    it('refuses folders and an empty name, with a message, and keeps Export disabled', async () => {
      const user = await openDialog();
      const d = within(dialog());
      await user.click(d.getByRole('button', { name: 'Choose folder…' }));
      await d.findByText('/films/Library');
      const name = d.getByRole('textbox', { name: /File name/ });
      expect(d.getByRole('button', { name: 'Export' })).toBeEnabled();

      await user.clear(name);
      expect(await d.findByRole('alert')).toHaveTextContent('Type a file name.');
      expect(d.getByRole('button', { name: 'Export' })).toBeDisabled();

      await user.type(name, '../{roll}');
      expect(await d.findByRole('alert')).toHaveTextContent(
        'A file name cannot contain folders: remove the / or \\.',
      );
      expect(d.getByRole('button', { name: 'Export' })).toBeDisabled();
      expect(name).toHaveAttribute('aria-invalid', 'true');

      await user.clear(name);
      await user.type(name, 'Scan {{index:02}');
      expect(d.queryByRole('alert')).not.toBeInTheDocument();
      expect(d.getByText('Example: Scan 01.tif')).toBeInTheDocument();
    });

    it.each([
      ['{roll}_{index:03}', 'tiff16', 'Roll_001.tif'],
      ['{roll}-{index}', 'jpeg', 'Roll-1.jpg'],
      ['Scan {index:05}.TIF', 'tiff16', 'Scan 00001.TIF'],
      ['plain', 'jpeg', 'plain.jpg'],
      ['{index:02}.{ext}', 'jpeg', '01.jpg'],
    ] as const)('shows %s as %s', (template, format, shown) => {
      expect(exampleName(template, format)).toBe(shown);
    });

    it('shows no example for a name it cannot make', () => {
      for (const template of ['{nope}', '{', '', '   ', '{index:0}']) {
        expect(exampleName(template, 'jpeg')).toBeNull();
      }
    });
  });

  describe('running an export', () => {
    it('starts the job with the chosen options and follows it to a summary', async () => {
      const user = await openDialog();
      const d = within(dialog());
      await user.click(d.getByRole('radio', { name: /JPEG/ }));
      await user.selectOptions(d.getByRole('combobox', { name: /Colour space/ }), 'adobe_rgb');
      await start(user);

      expect(state.exports).toEqual([
        {
          frameIds: ['summer-f00001', 'summer-f00002'],
          options: {
            format: 'jpeg',
            space: 'adobe_rgb',
            jpeg_quality: 92,
            dest_dir: '/films/Library',
            template: '{roll}_{index:03}',
            write_metadata: true,
          },
        },
      ]);

      act(() =>
        listeners.progress.forEach((cb) =>
          cb({ id: 'job-export-1', stage: 'exporting', fraction: 0.5 }),
        ),
      );
      expect(d.getByRole('progressbar', { name: 'Export 2 frames' })).toHaveAttribute(
        'value',
        '50',
      );
      expect(d.getByText('Closing this window does not stop the export.')).toBeInTheDocument();

      finish({
        files: [
          { frame_id: 'summer-f00001', path: '/films/Library/Summer_001.jpg' },
          { frame_id: 'summer-f00002', path: '/films/Library/Summer_002.jpg' },
        ],
        failed: [],
      });
      expect(await d.findByText('2 files written to /films/Library.')).toBeInTheDocument();
      expect(d.getByText('Done')).toBeInTheDocument();
      expect(d.getByText('2 files written')).toBeInTheDocument();
      expect(d.queryByText(/could not be exported/)).not.toBeInTheDocument();
    });

    it('sends write_metadata off when the box is cleared, and remembers it', async () => {
      const user = await openDialog();
      const d = within(dialog());
      await user.click(d.getByRole('checkbox', { name: 'Write metadata' }));
      expect(d.getByRole('checkbox', { name: 'Write metadata' })).not.toBeChecked();
      await start(user);
      expect(state.exports[0]?.options.write_metadata).toBe(false);
      expect(state.lastExport?.options.write_metadata).toBe(false);
    });

    it('lists the frames that could not be exported beside what was written', async () => {
      const user = await openDialog();
      await start(user);
      finish({
        files: [{ frame_id: 'summer-f00001', path: '/films/Library/Summer_001.tif' }],
        failed: [
          { frame_id: 'summer-f00002', message: 'This frame has no scan file.', details: null },
        ],
      });
      const d = within(dialog());
      expect(await d.findByText('1 file written to /films/Library.')).toBeInTheDocument();
      expect(d.getByText('1 file written, 1 frame could not be exported')).toBeInTheDocument();
      await user.click(d.getByText('Frames that could not be exported'));
      expect(d.getByText('This frame has no scan file.')).toBeInTheDocument();
    });

    it('cancels a running export, and says the files already written stay', async () => {
      const user = await openDialog();
      await start(user);
      const d = within(dialog());
      await user.click(d.getByRole('button', { name: 'Cancel' }));
      expect(ipc.jobCancel).toHaveBeenCalledWith('job-export-1');
      act(() =>
        listeners.error.forEach((cb) =>
          cb({
            id: 'job-export-1',
            error: { code: 'Cancelled', message: 'The job was cancelled.' },
          }),
        ),
      );
      expect(await d.findByText('Cancelled')).toBeInTheDocument();
      expect(d.getByText('files already written stay')).toBeInTheDocument();
    });

    it('shows a failed job with the backend’s message', async () => {
      const user = await openDialog();
      await start(user);
      act(() =>
        listeners.error.forEach((cb) =>
          cb({ id: 'job-export-1', error: { code: 'Io', message: 'The disk is full.' } }),
        ),
      );
      const d = within(dialog());
      expect(await d.findByText('Failed')).toBeInTheDocument();
      expect(d.getByText('The disk is full.')).toBeInTheDocument();
    });

    it('says why an export could not be started, and stays on the form', async () => {
      state.exportRefused = 'JPEG quality runs from 1 to 100.';
      const user = await openDialog();
      const d = within(dialog());
      await user.click(d.getByRole('button', { name: 'Choose folder…' }));
      await d.findByText('/films/Library');
      await user.click(d.getByRole('button', { name: 'Export' }));
      expect(await d.findByRole('alert')).toHaveTextContent(
        'The export could not be started: JPEG quality runs from 1 to 100.',
      );
      expect(d.getByRole('button', { name: 'Export' })).toBeEnabled();
      expect(useLibraryStore.getState().jobs).toHaveLength(0);
    });

    it('goes back to the form to export more, and the job stays in the Jobs panel', async () => {
      const user = await openDialog();
      await start(user);
      finish({ files: [], failed: [] });
      const d = within(dialog());
      await user.click(await d.findByRole('button', { name: 'Export more' }));
      expect(d.getByRole('button', { name: 'Choose folder…' })).toBeInTheDocument();
      // The same options are still there.
      expect(d.getByText('/films/Library')).toBeInTheDocument();
      await user.click(d.getByRole('button', { name: 'Close' }));
      const jobs = await screen.findByRole('list', { name: 'Background jobs' });
      expect(within(jobs).getByText('Export 2 frames')).toBeInTheDocument();
    });
  });

  describe('closing it', () => {
    it('does not stop the export, and reopening shows it', async () => {
      const user = await openDialog();
      await start(user);
      await user.keyboard('{Escape}');
      await waitFor(() =>
        expect(screen.queryByRole('dialog', { name: 'Export' })).not.toBeInTheDocument(),
      );
      expect(ipc.jobCancel).not.toHaveBeenCalled();
      const jobs = screen.getByRole('list', { name: 'Background jobs' });
      expect(within(jobs).getByRole('button', { name: 'Cancel' })).toBeInTheDocument();

      act(() =>
        listeners.progress.forEach((cb) =>
          cb({ id: 'job-export-1', stage: 'exporting', fraction: 0.25 }),
        ),
      );
      await user.click(exportButton());
      const again = within(await screen.findByRole('dialog', { name: 'Export' }));
      expect(await again.findByRole('progressbar', { name: 'Export 2 frames' })).toHaveAttribute(
        'value',
        '25',
      );
      expect(again.queryByRole('button', { name: 'Choose folder…' })).not.toBeInTheDocument();
    });
  });

  describe('remembering the options in the settings', () => {
    it('keeps what was used last time for the next time', async () => {
      const user = await openDialog();
      const d = within(dialog());
      await user.click(d.getByRole('radio', { name: /JPEG/ }));
      await user.selectOptions(d.getByRole('combobox', { name: /Colour space/ }), 'adobe_rgb');
      const name = d.getByRole('textbox', { name: /File name/ });
      await user.clear(name);
      await user.type(name, 'Frame {{index}');
      await start(user);
      finish({ files: [], failed: [] });
      await user.click(await d.findByRole('button', { name: 'Close' }));
      act(() => useLibraryStore.getState().dismissJob('job-export-1'));

      await user.click(exportButton());
      const next = within(await screen.findByRole('dialog', { name: 'Export' }));
      await waitFor(() => expect(next.getByRole('radio', { name: /JPEG/ })).toBeChecked());
      expect(next.getByRole('combobox', { name: /Colour space/ })).toHaveValue('adobe_rgb');
      expect(next.getByRole('textbox', { name: /File name/ })).toHaveValue('Frame {index}');
      expect(next.getByText('/films/Library')).toBeInTheDocument();
    });
  });

  it('has no accessibility violations: the form, a running export and a finished one', async () => {
    const user = await openDialog();
    await expectNoA11yViolations(document.body);
    await user.click(within(dialog()).getByRole('radio', { name: /JPEG/ }));
    await expectNoA11yViolations(document.body);
    await start(user);
    await expectNoA11yViolations(document.body);
    finish({
      files: [{ frame_id: 'summer-f00001', path: '/films/Library/Summer_001.jpg' }],
      failed: [{ frame_id: 'summer-f00002', message: 'No scan.', details: null }],
    });
    await within(dialog()).findByText('1 file written to /films/Library.');
    await expectNoA11yViolations(document.body);
  });
});
