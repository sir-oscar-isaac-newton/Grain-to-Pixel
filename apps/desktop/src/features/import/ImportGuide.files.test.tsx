import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import { expectNoA11yViolations } from '../../test-support';
import type { TripletCandidate } from '../../shared/ipc';
import { ipc, libraryDetails, listeners, makeRoll, resetFake, state } from '../library/fake-ipc';
import { resetLibraryStore, useLibraryStore } from '../library/library-store';
import { useLibraryEvents } from '../library/use-library-events';
import { resetGuideStore, useGuideStore } from './guide-store';
import { ImportView } from './ImportView';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

type User = ReturnType<typeof userEvent.setup>;

/** The Import view with the job events running, as in the app. */
function Guide() {
  useLibraryEvents(true);
  return <ImportView />;
}

const stepBar = () => screen.getByRole('navigation', { name: 'Import steps' });
const current = () => within(stepBar()).getByRole('button', { current: 'step' });
const stepButton = (name: RegExp | string) => within(stepBar()).getByRole('button', { name });
const next = () => screen.getByRole('button', { name: /^(Next|Finish)$/ });
const back = () => screen.getByRole('button', { name: 'Back' });
const kind = (name: string) => screen.getByRole('radio', { name });

const shot = (name: string, n: number) => ({
  path: `C:\\film\\${name}.NEF`,
  captured: `2026-10-06T19:43:0${n}.200`,
  channel: null,
});
const whole = (a: string, b: string, c: string, n: number): TripletCandidate => ({
  files: [shot(a, n), shot(b, n + 1), shot(c, n + 2)],
  problem: null,
});
const BROKEN: TripletCandidate = {
  files: [shot('DSC_0004', 7), shot('DSC_0005', 8)],
  problem: { kind: 'missing_shots', missing: 1 },
};
const TIFFS = ['C:\\scans\\harbour-01.tif', 'C:\\scans\\harbour-02.tif'];

function openLibrary(...rolls: ReturnType<typeof makeRoll>[]) {
  state.details = libraryDetails();
  state.rolls = rolls;
  useLibraryStore.setState({ phase: 'ready', library: state.details, rolls, rollId: null });
}

/** Opens a library with a roll, chooses the roll and Files on the Roll page and goes to the Files step. */
async function begin(user: User, roll = makeRoll({ name: 'Harbour' })) {
  openLibrary(roll);
  const view = render(<Guide />);
  await user.click(await screen.findByRole('radio', { name: 'Harbour' }));
  await user.click(screen.getByRole('radio', { name: 'Files' }));
  await user.click(next());
  await screen.findByRole('heading', { name: 'Choose the files' });
  return { ...view, roll };
}

async function chooseCamera(user: User) {
  await user.click(screen.getByRole('button', { name: 'Choose camera files…' }));
  await screen.findByRole('list', { name: 'Triplets found' });
}

async function chooseTiff(user: User) {
  await user.click(screen.getByRole('button', { name: 'Choose TIFF scans…' }));
  await screen.findByRole('list', { name: 'TIFF scans chosen' });
}

describe('Import guide: the files steps', () => {
  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetGuideStore();
    useUiStore.setState({ view: 'import' });
    state.pickedCamera = ['C:\\film\\DSC_0001.NEF'];
  });

  describe('Files', () => {
    it('offers camera files and TIFF scans, camera files first', async () => {
      const user = userEvent.setup();
      await begin(user);
      expect(screen.getByRole('heading', { name: 'Choose the files' })).toHaveFocus();
      const group = screen.getByRole('group', { name: 'What are the files?' });
      expect(within(group).getAllByRole('radio').length).toBe(2);
      expect(kind('Camera files (RGB triplets)')).toBeChecked();
      expect(kind('TIFF scans')).not.toBeChecked();
      expect(screen.getByRole('button', { name: 'Choose camera files…' })).toBeEnabled();
      expect(next()).toBeDisabled();
      expect(within(current()).queryByText('Done')).not.toBeInTheDocument();
    });

    it('is done with at least one whole triplet confirmed, and not before', async () => {
      const user = userEvent.setup();
      state.triplets = [whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0), BROKEN];
      await begin(user);
      await chooseCamera(user);
      expect(screen.getByText(/1 triplet ready to import/)).toHaveTextContent(
        '1 group needs attention',
      );
      expect(within(current()).getByText('Done')).toBeInTheDocument();
      expect(next()).toBeEnabled();

      await user.click(screen.getByRole('checkbox', { name: 'Import triplet 1' }));
      expect(next()).toBeDisabled();
      expect(within(current()).queryByText('Done')).not.toBeInTheDocument();
      expect(stepButton('Import')).toBeDisabled();
      await user.click(screen.getByRole('checkbox', { name: 'Import triplet 1' }));
      expect(next()).toBeEnabled();
    });

    it('is not done when every group has a problem', async () => {
      const user = userEvent.setup();
      state.triplets = [BROKEN];
      await begin(user);
      await chooseCamera(user);
      expect(screen.getByRole('alert')).toHaveTextContent('1 shot is missing');
      expect(next()).toBeDisabled();
    });

    it('keeps the order of a triplet that the user fixes, and sends it in that order', async () => {
      const user = userEvent.setup();
      state.triplets = [whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0)];
      const { roll } = await begin(user);
      await chooseCamera(user);
      await user.click(screen.getByRole('button', { name: 'Move DSC_0001.NEF later' }));
      await user.click(next());
      await user.click(screen.getByRole('button', { name: 'Import 1 triplet' }));
      expect(ipc.importFiles).toHaveBeenCalledWith(
        ['C:\\film\\DSC_0002.NEF', 'C:\\film\\DSC_0001.NEF', 'C:\\film\\DSC_0003.NEF'],
        roll.id,
        'RgbTriplet',
      );
    });

    it('chooses TIFF scans as a plain list of single files, one frame each', async () => {
      const user = userEvent.setup();
      ipc.pickTiffFiles.mockResolvedValue(TIFFS);
      await begin(user);
      await user.click(kind('TIFF scans'));
      expect(
        screen.queryByRole('button', { name: 'Choose camera files…' }),
      ).not.toBeInTheDocument();
      expect(next()).toBeDisabled();
      await chooseTiff(user);

      expect(ipc.tripletGroup).not.toHaveBeenCalled();
      expect(screen.getByText('2 TIFF scans ready to import')).toBeInTheDocument();
      const list = screen.getByRole('list', { name: 'TIFF scans chosen' });
      expect(
        within(list)
          .getAllByRole('listitem')
          .map((li) => li.textContent),
      ).toEqual(['harbour-01.tifRemove', 'harbour-02.tifRemove']);
      // No triplets, no confirm boxes, no channels.
      expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
      expect(within(current()).getByText('Done')).toBeInTheDocument();
      expect(next()).toBeEnabled();
    });

    it('removes a TIFF scan from the list, and is not done once the list is empty', async () => {
      const user = userEvent.setup();
      ipc.pickTiffFiles.mockResolvedValue(TIFFS);
      await begin(user);
      await user.click(kind('TIFF scans'));
      await chooseTiff(user);
      await user.click(screen.getByRole('button', { name: 'Remove harbour-01.tif' }));
      expect(screen.getByText('1 TIFF scan ready to import')).toBeInTheDocument();
      expect(screen.queryByText('harbour-01.tif')).not.toBeInTheDocument();
      expect(next()).toBeEnabled();
      await user.click(screen.getByRole('button', { name: 'Remove harbour-02.tif' }));
      expect(screen.queryByRole('list', { name: 'TIFF scans chosen' })).not.toBeInTheDocument();
      expect(next()).toBeDisabled();
    });

    it('chooses TIFF scans again, which replaces the list, and keeps it when the picker is cancelled', async () => {
      const user = userEvent.setup();
      ipc.pickTiffFiles.mockResolvedValueOnce(TIFFS);
      await begin(user);
      await user.click(kind('TIFF scans'));
      await chooseTiff(user);

      ipc.pickTiffFiles.mockResolvedValueOnce([]);
      await user.click(screen.getByRole('button', { name: 'Choose TIFF scans…' }));
      expect(screen.getByText('2 TIFF scans ready to import')).toBeInTheDocument();

      ipc.pickTiffFiles.mockResolvedValueOnce(['C:\\scans\\beach-07.tif']);
      await user.click(screen.getByRole('button', { name: 'Choose TIFF scans…' }));
      expect(await screen.findByText('1 TIFF scan ready to import')).toBeInTheDocument();
      expect(screen.getByText('beach-07.tif')).toBeInTheDocument();
      expect(screen.queryByText('harbour-01.tif')).not.toBeInTheDocument();
    });

    it('says why the files could not be chosen or grouped', async () => {
      const user = userEvent.setup();
      state.tripletsFail = 'The disk is not there.';
      await begin(user);
      await user.click(screen.getByRole('button', { name: 'Choose camera files…' }));
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The files could not be grouped: The disk is not there.',
      );
      expect(next()).toBeDisabled();

      ipc.pickTiffFiles.mockRejectedValueOnce(new Error('The dialog failed.'));
      await user.click(kind('TIFF scans'));
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Choose TIFF scans…' }));
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The files could not be chosen: The dialog failed.',
      );
    });

    it('keeps each kind of file when the user switches, and only the chosen kind counts', async () => {
      const user = userEvent.setup();
      state.triplets = [whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0)];
      ipc.pickTiffFiles.mockResolvedValue(TIFFS);
      await begin(user);
      await chooseCamera(user);
      await user.click(kind('TIFF scans'));
      // Nothing is chosen of this kind yet, whatever the camera files are.
      expect(next()).toBeDisabled();
      await chooseTiff(user);
      expect(next()).toBeEnabled();
      await user.click(kind('Camera files (RGB triplets)'));
      expect(screen.getByRole('list', { name: 'Triplets found' })).toBeInTheDocument();
      expect(next()).toBeEnabled();
      await user.click(screen.getByRole('checkbox', { name: 'Import triplet 1' }));
      expect(next()).toBeDisabled();
      await user.click(kind('TIFF scans'));
      expect(screen.getByRole('list', { name: 'TIFF scans chosen' })).toBeInTheDocument();
      expect(next()).toBeEnabled();
    });

    it('leaves a read-only library alone', async () => {
      const user = userEvent.setup();
      state.details = libraryDetails({ read_only: true });
      const roll = makeRoll({ name: 'Harbour' });
      state.rolls = [roll];
      useLibraryStore.setState({
        phase: 'ready',
        library: state.details,
        rolls: [roll],
        rollId: null,
      });
      render(<Guide />);
      await user.click(await screen.findByRole('radio', { name: 'Harbour' }));
      await user.click(screen.getByRole('radio', { name: 'Files' }));
      await user.click(next());
      expect(screen.getByRole('button', { name: 'Choose camera files…' })).toBeDisabled();
      await user.click(kind('TIFF scans'));
      expect(screen.getByRole('button', { name: 'Choose TIFF scans…' })).toBeDisabled();
    });
  });

  describe('Import', () => {
    it('imports TIFF scans as TiffScan, follows the job and clears the files', async () => {
      const user = userEvent.setup();
      ipc.pickTiffFiles.mockResolvedValue(TIFFS);
      ipc.importFiles.mockResolvedValueOnce('job-tiff');
      const { roll } = await begin(user);
      await user.click(kind('TIFF scans'));
      await chooseTiff(user);
      await user.click(next());

      expect(screen.getByRole('heading', { name: 'Import the files' })).toHaveFocus();
      expect(screen.getByText('2 TIFF scans ready to import')).toBeInTheDocument();
      expect(next()).toHaveTextContent('Finish');
      await user.click(screen.getByRole('button', { name: 'Import 2 TIFF scans' }));
      expect(ipc.importFiles).toHaveBeenCalledWith(TIFFS, roll.id, 'TiffScan');

      expect(await screen.findByText(/The import started/)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Import 0 TIFF scans' })).toBeDisabled();
      expect(await screen.findByRole('progressbar', { name: 'Import into Harbour' })).toBeVisible();
      act(() =>
        listeners.progress.forEach((cb) =>
          cb({ id: 'job-tiff', stage: 'importing', fraction: 0.5 }),
        ),
      );
      await user.click(screen.getByRole('button', { name: 'Cancel' }));
      expect(ipc.jobCancel).toHaveBeenCalledWith('job-tiff');

      act(() =>
        listeners.done.forEach((cb) =>
          cb({
            id: 'job-tiff',
            result: {
              kind: 'Imported',
              frames: ['frame-a', 'frame-b'],
              frame_ms: [5, 5],
              skipped: [],
              warnings: [],
            } as never,
          }),
        ),
      );
      await waitFor(() => expect(screen.getByText(/2 frames imported/)).toBeInTheDocument());

      // The chosen files are gone: back on Files nothing is chosen.
      await user.click(back());
      expect(screen.queryByRole('list', { name: 'TIFF scans chosen' })).not.toBeInTheDocument();
      expect(kind('TIFF scans')).toBeChecked();
      expect(next()).toBeDisabled();
    });

    it('imports camera files as RgbTriplet, in the roll of the guide', async () => {
      const user = userEvent.setup();
      state.triplets = [
        whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0),
        whole('DSC_0004', 'DSC_0005', 'DSC_0006', 10),
      ];
      const { roll } = await begin(user);
      await chooseCamera(user);
      await user.click(screen.getByRole('checkbox', { name: 'Import triplet 2' }));
      await user.click(next());
      expect(screen.getByText('1 triplet ready to import')).toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Import 1 triplet' }));
      expect(ipc.importFiles).toHaveBeenCalledWith(
        ['C:\\film\\DSC_0001.NEF', 'C:\\film\\DSC_0002.NEF', 'C:\\film\\DSC_0003.NEF'],
        roll.id,
        'RgbTriplet',
      );
    });

    it('says why an import could not start, and keeps the files to try again', async () => {
      const user = userEvent.setup();
      ipc.pickTiffFiles.mockResolvedValue(TIFFS);
      ipc.importFiles.mockRejectedValueOnce(new Error('The library is read-only.'));
      await begin(user);
      await user.click(kind('TIFF scans'));
      await chooseTiff(user);
      await user.click(next());
      await user.click(screen.getByRole('button', { name: 'Import 2 TIFF scans' }));
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The import could not be started: The library is read-only.',
      );
      expect(screen.queryByText(/The import started/)).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Import 2 TIFF scans' })).toBeEnabled();
    });

    it('says that no files are chosen when the step is opened with none', async () => {
      const roll = makeRoll({ name: 'Harbour' });
      openLibrary(roll);
      state.importGuide[roll.id] = { source: 'files', step: 'import' };
      useGuideStore.getState().setLastRoll(roll.id);
      render(<Guide />);
      expect(await screen.findByRole('heading', { name: 'Import the files' })).toBeInTheDocument();
      expect(
        screen.getByText('No files are chosen. Go back to Files to choose some.'),
      ).toBeVisible();
      expect(screen.getByRole('button', { name: 'Import 0 triplets' })).toBeDisabled();
      expect(next()).toHaveTextContent('Finish');
      expect(next()).toBeEnabled();
    });

    it('ends with Finish, which opens the roll in the Library', async () => {
      const user = userEvent.setup();
      state.triplets = [whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0)];
      const { roll } = await begin(user);
      await chooseCamera(user);
      await user.click(next());
      expect(next()).toHaveTextContent('Finish');
      await user.click(next());
      expect(useUiStore.getState().view).toBe('library');
      expect(useLibraryStore.getState().rollId).toBe(roll.id);
    });
  });

  describe('Moving between steps', () => {
    it('keeps the files, the confirmations and the kind', async () => {
      const user = userEvent.setup();
      state.triplets = [
        whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0),
        whole('DSC_0004', 'DSC_0005', 'DSC_0006', 10),
      ];
      ipc.pickTiffFiles.mockResolvedValue(TIFFS);
      await begin(user);
      await chooseCamera(user);
      await user.click(screen.getByRole('checkbox', { name: 'Import triplet 2' }));
      await user.click(kind('TIFF scans'));
      await chooseTiff(user);
      await user.click(next());
      await user.click(stepButton(/^Roll/));
      await user.click(stepButton(/^Files/));

      expect(kind('TIFF scans')).toBeChecked();
      expect(screen.getByRole('list', { name: 'TIFF scans chosen' })).toBeInTheDocument();
      await user.click(kind('Camera files (RGB triplets)'));
      expect(screen.getByRole('checkbox', { name: 'Import triplet 1' })).toBeChecked();
      expect(screen.getByRole('checkbox', { name: 'Import triplet 2' })).not.toBeChecked();
      expect(ipc.tripletGroup).toHaveBeenCalledTimes(1);
      expect(ipc.pickTiffFiles).toHaveBeenCalledTimes(1);
    });

    it('starts afresh for another roll', async () => {
      const user = userEvent.setup();
      state.triplets = [whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0)];
      ipc.pickTiffFiles.mockResolvedValue(TIFFS);
      openLibrary(
        makeRoll({ name: 'Harbour', created_at: '2026-01-01T10:00:00Z' }),
        makeRoll({ name: 'Beach', created_at: '2026-06-01T10:00:00Z' }),
      );
      render(<Guide />);
      await user.click(await screen.findByRole('radio', { name: 'Harbour' }));
      await user.click(screen.getByRole('radio', { name: 'Files' }));
      await user.click(next());
      await user.click(kind('TIFF scans'));
      await chooseTiff(user);

      await user.click(stepButton(/^Roll/));
      await user.click(screen.getByRole('radio', { name: /^Beach/ }));
      await user.click(next());
      expect(kind('Camera files (RGB triplets)')).toBeChecked();
      expect(next()).toBeDisabled();
      await user.click(kind('TIFF scans'));
      expect(screen.queryByRole('list', { name: 'TIFF scans chosen' })).not.toBeInTheDocument();
    });

    it('drops the files when the guide leaves the files source', async () => {
      const user = userEvent.setup();
      ipc.pickTiffFiles.mockResolvedValue(TIFFS);
      await begin(user);
      await user.click(kind('TIFF scans'));
      await chooseTiff(user);
      await user.click(stepButton(/^Roll/));
      await user.click(screen.getByRole('radio', { name: 'Scanner' }));
      await user.click(screen.getByRole('radio', { name: 'Files' }));
      await user.click(next());
      expect(kind('Camera files (RGB triplets)')).toBeChecked();
      expect(next()).toBeDisabled();
    });
  });

  describe('Resuming', () => {
    it('goes no further than Files for a roll left at Import, as no files are chosen', async () => {
      const user = userEvent.setup();
      const roll = makeRoll({ name: 'Harbour' });
      openLibrary(roll);
      state.importGuide[roll.id] = { source: 'files', step: 'import' };
      render(<Guide />);
      const choice = await screen.findByRole('radio', { name: /^Harbour/ });
      expect(choice).toHaveAccessibleName(expect.stringContaining('Resume at Import'));
      await user.click(choice);
      expect(screen.getByRole('radio', { name: 'Files' })).toBeChecked();
      await user.click(next());
      expect(await screen.findByRole('heading', { name: 'Choose the files' })).toBeInTheDocument();
      expect(current()).toHaveTextContent('Files');
    });

    it('saves the step of the roll as the guide moves', async () => {
      const user = userEvent.setup();
      state.triplets = [whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0)];
      const { roll } = await begin(user);
      await chooseCamera(user);
      await user.click(next());
      const saved = state.settingsPatches.flatMap((p) =>
        p.import_guide_entry ? [p.import_guide_entry] : [],
      );
      expect(saved.map((s) => s.entry?.step)).toEqual(['roll', 'files', 'import']);
      expect(saved.every((s) => s.roll_id === roll.id && s.entry?.source === 'files')).toBe(true);
    });
  });

  it('has no accessibility violations on any of the files steps', async () => {
    const user = userEvent.setup();
    state.triplets = [whole('DSC_0001', 'DSC_0002', 'DSC_0003', 0), BROKEN];
    ipc.pickTiffFiles.mockResolvedValue(TIFFS);
    const { container } = await begin(user);
    await expectNoA11yViolations(container);
    await chooseCamera(user);
    await expectNoA11yViolations(container);
    await user.click(kind('TIFF scans'));
    await chooseTiff(user);
    await expectNoA11yViolations(container);

    await user.click(next());
    await screen.findByRole('heading', { name: 'Import the files' });
    await expectNoA11yViolations(container);
    ipc.importFiles.mockResolvedValueOnce('job-a11y');
    await user.click(screen.getByRole('button', { name: 'Import 2 TIFF scans' }));
    await screen.findByRole('progressbar', { name: 'Import into Harbour' });
    await expectNoA11yViolations(container);
    await screen.findByText(/The import started/);
    await expectNoA11yViolations(container);
  });
});
