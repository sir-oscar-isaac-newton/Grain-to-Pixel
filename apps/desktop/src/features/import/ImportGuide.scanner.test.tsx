import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import { expectNoA11yViolations } from '../../test-support';
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
const field = (name: string) => screen.getByRole('spinbutton', { name });
const ir = () => screen.getByRole('checkbox', { name: /infrared/ });
/** The scan button of the Scan step (the step bar has a button of the same name). */
const startScan = () =>
  within(screen.getByRole('region', { name: 'Scan the roll' })).getByRole('button', {
    name: /^Scan( frames?|$)/,
  });

const FOUND = 'Scanner: Nikon LS-50 ED.';

function openLibrary(...rolls: ReturnType<typeof makeRoll>[]) {
  state.details = libraryDetails();
  state.rolls = rolls;
  useLibraryStore.setState({ phase: 'ready', library: state.details, rolls, rollId: null });
}

/** Chooses the roll and the scanner on the Roll page, and goes to the Scanner step. */
async function startScanner(user: User, name = 'Harbour') {
  await user.click(await screen.findByRole('radio', { name }));
  await user.click(screen.getByRole('radio', { name: 'Scanner' }));
  await user.click(next());
  await screen.findByRole('heading', { name: 'Find the scanner' });
}

/** Opens a library with a roll and walks the guide to the named step (every step before it must pass). */
async function begin(user: User, step: string, over: Parameters<typeof makeRoll>[0] = {}) {
  const roll = makeRoll({ name: 'Harbour', ...over });
  openLibrary(roll);
  const view = render(<Guide />);
  await startScanner(user);
  await screen.findByText(FOUND);
  // "Scan" is also the start of "Scanner".
  const at = new RegExp(`^${step}(?![a-z])`);
  for (let i = 0; i < 4 && !at.test(current().textContent ?? ''); i += 1) {
    expect(next()).toBeEnabled();
    await user.click(next());
  }
  expect(current()).toHaveTextContent(at);
  return { ...view, roll };
}

describe('Import guide: the scanner steps', () => {
  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetGuideStore();
    useUiStore.setState({ view: 'import' });
  });

  describe('Scanner', () => {
    it('is done once a scanner is found, and not before', async () => {
      const user = userEvent.setup();
      openLibrary(makeRoll({ name: 'Harbour' }));
      state.devices = [];
      render(<Guide />);
      await startScanner(user);
      expect(screen.getByRole('heading', { name: 'Find the scanner' })).toHaveFocus();
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'No scanner was found. Check that it is on and connected.',
      );
      expect(next()).toBeDisabled();
      expect(within(current()).queryByText('Done')).not.toBeInTheDocument();

      state.devices = [{ id: 'usb:1', name: 'Nikon LS-50 ED' }];
      await user.click(screen.getByRole('button', { name: 'Look again' }));
      expect(await screen.findByText(FOUND)).toBeInTheDocument();
      expect(next()).toBeEnabled();
      expect(within(current()).getByText('Done')).toBeInTheDocument();
    });

    it('says why there is no scanner, with the hint about the driver', async () => {
      const user = userEvent.setup();
      openLibrary(makeRoll({ name: 'Harbour' }));
      state.devicesFail = 'The scanner has the wrong driver.';
      render(<Guide />);
      await startScanner(user);
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The scanner has the wrong driver.',
      );
      expect(screen.getByText(/WinUSB driver/)).toBeInTheDocument();
      expect(next()).toBeDisabled();
    });
  });

  describe('Frames', () => {
    it('is done with a valid range, and the range is checked in words', async () => {
      const user = userEvent.setup();
      await begin(user, 'Frames');
      expect(screen.getByRole('heading', { name: 'Choose the frames' })).toHaveFocus();
      expect(field('First frame')).toHaveValue(1);
      expect(field('Last frame')).toHaveValue(40);
      expect(screen.getByText('No frame of this roll has been scanned yet.')).toBeInTheDocument();
      expect(within(current()).getByText('Done')).toBeInTheDocument();
      expect(next()).toBeEnabled();

      await user.clear(field('First frame'));
      await user.type(field('First frame'), '0');
      expect(screen.getByText('A frame is a whole number from 1 to 40.')).toBeInTheDocument();
      expect(within(current()).queryByText('Done')).not.toBeInTheDocument();
      expect(next()).toBeDisabled();
      // A step after Frames cannot be opened from the bar while the range is not valid.
      expect(stepButton('Settings')).toBeDisabled();

      await user.clear(field('First frame'));
      await user.type(field('First frame'), '12');
      await user.clear(field('Last frame'));
      await user.type(field('Last frame'), '9');
      expect(
        screen.getByText('The last frame must not come before the first.'),
      ).toBeInTheDocument();
      expect(next()).toBeDisabled();

      await user.clear(field('Last frame'));
      await user.type(field('Last frame'), '12');
      expect(next()).toBeEnabled();
      expect(within(current()).getByText('Done')).toBeInTheDocument();
    });

    it('continues at the next frame after a stop, and can start again from frame 1', async () => {
      const user = userEvent.setup();
      state.resume = 6;
      const { roll } = await begin(user, 'Frames');
      expect(await screen.findByText(/Continue at frame 6/)).toBeInTheDocument();
      expect(ipc.scanResumePoint).toHaveBeenCalledWith(roll.id);
      expect(field('First frame')).toHaveValue(6);
      await user.click(screen.getByRole('button', { name: 'Start from frame 1' }));
      expect(field('First frame')).toHaveValue(1);
    });

    it('says when every frame has been scanned, and lets the user pass to Finish', async () => {
      const user = userEvent.setup();
      state.resume = 41;
      await begin(user, 'Frames');
      expect(
        await screen.findByText('All 40 frames of this roll have been scanned.'),
      ).toBeInTheDocument();
      expect(within(current()).getByText('Done')).toBeInTheDocument();

      // Even with the range spoilt there is nothing left to choose, so Next stays on.
      await user.clear(field('First frame'));
      expect(next()).toBeEnabled();
      await user.click(next());
      await user.click(next());
      expect(current()).toHaveTextContent(/^Scan$/);
      expect(startScan()).toBeDisabled();
      expect(next()).toHaveTextContent('Finish');
      expect(next()).toBeEnabled();
    });

    it('previews the roll as a strip with its frame count', async () => {
      const user = userEvent.setup();
      state.previewImage = {
        frame_count: 36,
        pitch: 143,
        strip_width: 5149,
        strip_height: 96,
        strip_jpeg: [0xff, 0xd8, 0xff, 0xd9],
      };
      await begin(user, 'Frames');
      await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
      expect(
        await screen.findByRole('img', { name: /36 frames side by side/ }),
      ).toBeInTheDocument();
      expect(screen.getByText(/36 frames found/)).toBeInTheDocument();
      expect(ipc.scanPreview).toHaveBeenCalledWith('usb:1');
    });
  });

  describe('Settings', () => {
    it('has no state: Next is on at once, with the defaults', async () => {
      const user = userEvent.setup();
      await begin(user, 'Settings');
      expect(screen.getByRole('heading', { name: 'Scan settings' })).toHaveFocus();
      expect(screen.getByRole('combobox', { name: 'Resolution' })).toHaveValue('4000');
      expect(screen.getByRole('combobox', { name: 'Samples per pixel' })).toHaveValue('1');
      expect(next()).toBeEnabled();
      expect(within(current()).queryByText('Done')).not.toBeInTheDocument();
    });

    it.each([
      ['bw_silver', false],
      ['color_negative', true],
      ['bw_chromogenic', true],
    ] as const)('starts a %s roll with the infrared %s', async (type, on) => {
      const user = userEvent.setup();
      await begin(user, 'Settings', { film_type: type });
      if (on) expect(ir()).toBeChecked();
      else expect(ir()).not.toBeChecked();
    });

    it('lets the user change the infrared, and keeps the choice from step to step', async () => {
      const user = userEvent.setup();
      await begin(user, 'Settings', { film_type: 'bw_silver' });
      await user.click(ir());
      expect(ir()).toBeChecked();
      await user.click(back());
      await user.click(next());
      expect(ir()).toBeChecked();
    });
  });

  describe('Scan', () => {
    it('starts with the values of the earlier steps and follows the scan as a job', async () => {
      const user = userEvent.setup();
      const { roll } = await begin(user, 'Frames');
      await user.clear(field('Last frame'));
      await user.type(field('Last frame'), '3');
      await user.click(next());
      await user.selectOptions(screen.getByRole('combobox', { name: 'Resolution' }), '2000');
      await user.selectOptions(screen.getByRole('combobox', { name: 'Samples per pixel' }), '4');
      await user.click(ir());
      await user.click(next());
      expect(screen.getByRole('heading', { name: 'Scan the roll' })).toHaveFocus();
      expect(startScan()).toHaveTextContent('Scan frames 1 to 3');
      await user.click(startScan());

      await waitFor(() =>
        expect(ipc.scanStart).toHaveBeenCalledWith(roll.id, {
          device_id: 'usb:1',
          first_frame: 1,
          last_frame: 3,
          dpi: 2000,
          samples: 4,
          ir: false,
          autofocus: false,
        }),
      );
      const jobs = await screen.findByRole('list', { name: 'Background jobs' });
      expect(within(jobs).getByText('Scan into Harbour')).toBeInTheDocument();
      act(() =>
        listeners.progress.forEach((cb) =>
          cb({ id: 'job-scan', stage: 'scanning', fraction: 0.4 }),
        ),
      );
      expect(within(jobs).getByRole('progressbar', { name: 'Scan into Harbour' })).toHaveAttribute(
        'value',
        '40',
      );
      await user.click(within(jobs).getByRole('button', { name: /Cancel/ }));
      expect(ipc.jobCancel).toHaveBeenCalledWith('job-scan');
    });

    it('says why a scan could not start', async () => {
      const user = userEvent.setup();
      state.scanFail = 'The scanner is busy with another scan or a roll preview.';
      await begin(user, 'Scan');
      await user.click(startScan());
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The scan could not start: The scanner is busy with another scan or a roll preview.',
      );
      expect(screen.queryByRole('list', { name: 'Background jobs' })).not.toBeInTheDocument();
    });

    it('locks the inputs of Frames and Settings while a scan runs, and frees them after', async () => {
      const user = userEvent.setup();
      await begin(user, 'Scan');
      await user.click(startScan());
      await screen.findByRole('list', { name: 'Background jobs' });

      await user.click(stepButton(/^Settings/));
      expect(screen.getByRole('combobox', { name: 'Resolution' })).toBeDisabled();
      expect(screen.getByRole('combobox', { name: 'Samples per pixel' })).toBeDisabled();
      expect(ir()).toBeDisabled();
      await user.click(stepButton(/^Frames/));
      expect(field('First frame')).toBeDisabled();
      expect(field('Last frame')).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Preview the roll' })).toBeDisabled();

      state.resume = 3;
      act(() =>
        listeners.done.forEach((cb) =>
          cb({
            id: 'job-scan',
            result: {
              kind: 'Imported',
              frames: ['a', 'b'],
              frame_ms: [1, 1],
              warnings: [],
              skipped: [],
              base_is_guess: false,
              base_replaced: false,
              rebalanced: [],
            },
          }),
        ),
      );
      await waitFor(() => expect(field('First frame')).toBeEnabled());
      expect(await screen.findByText(/Continue at frame 3/)).toBeInTheDocument();
      expect(field('First frame')).toHaveValue(3);
    });

    it('ends with Finish, which opens the roll in the Library', async () => {
      const user = userEvent.setup();
      const { roll } = await begin(user, 'Scan');
      expect(next()).toHaveTextContent('Finish');
      expect(next()).toBeEnabled();
      await user.click(next());
      expect(useUiStore.getState().view).toBe('library');
      expect(useLibraryStore.getState().rollId).toBe(roll.id);
    });
  });

  describe('Moving between steps', () => {
    it('keeps the scanner, the range, the settings and the preview', async () => {
      const user = userEvent.setup();
      state.previewImage = {
        frame_count: 12,
        pitch: 100,
        strip_width: 1200,
        strip_height: 90,
        strip_jpeg: [0xff, 0xd8, 0xff, 0xd9],
      };
      await begin(user, 'Frames');
      await user.clear(field('First frame'));
      await user.type(field('First frame'), '4');
      await user.clear(field('Last frame'));
      await user.type(field('Last frame'), '9');
      await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
      await screen.findByRole('img', { name: /12 frames side by side/ });
      await user.click(next());
      await user.selectOptions(screen.getByRole('combobox', { name: 'Resolution' }), '1000');
      await user.selectOptions(screen.getByRole('combobox', { name: 'Samples per pixel' }), '8');
      ipc.scannerList.mockClear();
      ipc.scanResumePoint.mockClear();

      await user.click(stepButton(/^Scanner/));
      expect(screen.getByText(FOUND)).toBeInTheDocument();
      await user.click(stepButton(/^Frames/));
      expect(field('First frame')).toHaveValue(4);
      expect(field('Last frame')).toHaveValue(9);
      expect(screen.getByRole('img', { name: /12 frames side by side/ })).toBeInTheDocument();
      await user.click(stepButton(/^Settings/));
      expect(screen.getByRole('combobox', { name: 'Resolution' })).toHaveValue('1000');
      expect(screen.getByRole('combobox', { name: 'Samples per pixel' })).toHaveValue('8');
      expect(ipc.scannerList).not.toHaveBeenCalled();
      expect(ipc.scanResumePoint).not.toHaveBeenCalled();
    });

    it('starts afresh for another roll', async () => {
      const user = userEvent.setup();
      openLibrary(
        makeRoll({ name: 'Harbour', created_at: '2026-01-01T10:00:00Z' }),
        makeRoll({ name: 'Beach', created_at: '2026-06-01T10:00:00Z' }),
      );
      render(<Guide />);
      await startScanner(user);
      await screen.findByText(FOUND);
      await user.click(next());
      await user.clear(field('Last frame'));
      await user.type(field('Last frame'), '7');
      await user.click(next());
      await user.click(ir());
      expect(ir()).not.toBeChecked();

      await user.click(stepButton(/^Roll/));
      await user.click(screen.getByRole('radio', { name: /^Beach/ }));
      await user.click(next());
      await user.click(next());
      expect(field('Last frame')).toHaveValue(40);
      await user.click(next());
      expect(ir()).toBeChecked();
    });
  });

  describe('Leaving', () => {
    it('stops a roll preview that is still running when Import is left', async () => {
      const user = userEvent.setup();
      ipc.scanPreview.mockImplementationOnce(() => new Promise(() => undefined));
      const { unmount } = await begin(user, 'Frames');
      await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
      await screen.findByRole('button', { name: 'Stop the preview' });
      ipc.scanPreviewCancel.mockClear();
      unmount();
      expect(ipc.scanPreviewCancel).toHaveBeenCalledTimes(1);
    });

    it('does not look for the scanner while another source is chosen', async () => {
      const user = userEvent.setup();
      openLibrary(makeRoll({ name: 'Harbour' }));
      render(<Guide />);
      await user.click(await screen.findByRole('radio', { name: 'Harbour' }));
      await user.click(screen.getByRole('radio', { name: 'Files' }));
      await user.click(next());
      expect(ipc.scannerList).not.toHaveBeenCalled();
      expect(ipc.scanResumePoint).not.toHaveBeenCalled();
    });
  });

  describe('Resuming', () => {
    it('goes no further than Scanner for a roll left at Scan, as no scanner was looked for yet', async () => {
      const user = userEvent.setup();
      const roll = makeRoll({ name: 'Harbour' });
      openLibrary(roll);
      state.importGuide[roll.id] = { source: 'scanner', step: 'scan' };
      render(<Guide />);
      const choice = await screen.findByRole('radio', { name: /^Harbour/ });
      expect(choice).toHaveAccessibleName(expect.stringContaining('Resume at Scan'));
      await user.click(choice);
      expect(screen.getByRole('radio', { name: 'Scanner' })).toBeChecked();
      await user.click(next());
      expect(await screen.findByRole('heading', { name: 'Find the scanner' })).toBeInTheDocument();
      expect(current()).toHaveTextContent('Scanner');
    });

    it('stays at Scanner while there is none', async () => {
      const user = userEvent.setup();
      const roll = makeRoll({ name: 'Harbour' });
      openLibrary(roll);
      state.importGuide[roll.id] = { source: 'scanner', step: 'scan' };
      state.devices = [];
      render(<Guide />);
      await user.click(await screen.findByRole('radio', { name: /^Harbour/ }));
      await user.click(next());
      await screen.findByRole('alert');
      // Back on the Roll page and on again: still Scanner, as nothing was found.
      await user.click(back());
      await user.click(next());
      expect(current()).toHaveTextContent('Scanner');
      expect(next()).toBeDisabled();
    });

    it('goes back to the saved step of another roll once the scanner is known', async () => {
      const user = userEvent.setup();
      const harbour = makeRoll({ name: 'Harbour', created_at: '2026-01-01T10:00:00Z' });
      const beach = makeRoll({ name: 'Beach', created_at: '2026-06-01T10:00:00Z' });
      openLibrary(harbour, beach);
      state.importGuide[harbour.id] = { source: 'scanner', step: 'scan' };
      render(<Guide />);
      await startScanner(user, 'Beach');
      await screen.findByText(FOUND);
      await user.click(back());
      await user.click(screen.getByRole('radio', { name: /^Harbour/ }));
      await user.click(next());
      expect(current()).toHaveTextContent(/^Scan(?!ner)/);
      expect(ipc.scannerList).toHaveBeenCalledTimes(1);
    });

    it('opens the saved step of the roll worked on last in this session, and finds the scanner there', async () => {
      const roll = makeRoll({ name: 'Harbour' });
      openLibrary(roll);
      state.importGuide[roll.id] = { source: 'scanner', step: 'scan' };
      useGuideStore.getState().setLastRoll(roll.id);
      const user = userEvent.setup();
      render(<Guide />);
      expect(await screen.findByRole('heading', { name: 'Scan the roll' })).toBeInTheDocument();
      await waitFor(() => expect(ipc.scannerList).toHaveBeenCalledTimes(1));
      await waitFor(() => expect(startScan()).toBeEnabled());
      await user.click(startScan());
      await waitFor(() => expect(ipc.scanStart).toHaveBeenCalled());
    });
  });

  it('takes one roll from the Roll page to Finish', async () => {
    const user = userEvent.setup();
    const roll = makeRoll({ name: 'Harbour', film_type: 'bw_silver' });
    openLibrary(roll);
    render(<Guide />);
    await startScanner(user);
    await screen.findByText(FOUND);
    await user.click(next());
    expect(screen.getByRole('heading', { name: 'Choose the frames' })).toBeInTheDocument();
    await user.click(next());
    expect(screen.getByRole('heading', { name: 'Scan settings' })).toBeInTheDocument();
    expect(ir()).not.toBeChecked();
    await user.click(next());
    expect(screen.getByRole('heading', { name: 'Scan the roll' })).toBeInTheDocument();
    await user.click(startScan());
    await waitFor(() =>
      expect(ipc.scanStart).toHaveBeenCalledWith(
        roll.id,
        expect.objectContaining({ first_frame: 1, last_frame: 40, ir: false }),
      ),
    );
    await user.click(next());
    expect(useUiStore.getState().view).toBe('library');
    expect(useLibraryStore.getState().rollId).toBe(roll.id);
  });

  it('has no accessibility violations on any of the scanner steps', async () => {
    const user = userEvent.setup();
    openLibrary(makeRoll({ name: 'Harbour' }));
    const { container } = render(<Guide />);
    await startScanner(user);
    await screen.findByText(FOUND);
    await expectNoA11yViolations(container);
    for (const name of ['Choose the frames', 'Scan settings', 'Scan the roll']) {
      await user.click(next());
      await screen.findByRole('heading', { name });
      await expectNoA11yViolations(container);
    }
  });

  it('has no accessibility violations with no scanner, a preview, a range problem and a job', async () => {
    const user = userEvent.setup();
    state.devices = [];
    openLibrary(makeRoll({ name: 'Harbour' }));
    const none = render(<Guide />);
    await startScanner(user);
    await screen.findByRole('alert');
    await expectNoA11yViolations(none.container);
    none.unmount();
    resetGuideStore();

    state.devices = [{ id: 'usb:1', name: 'Nikon LS-50 ED' }];
    openLibrary(makeRoll({ name: 'Harbour' }));
    const view = render(<Guide />);
    await startScanner(user);
    await screen.findByText(FOUND);
    await user.click(next());
    await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
    await screen.findByRole('img', { name: /frames side by side/ });
    await user.clear(field('First frame'));
    await expectNoA11yViolations(view.container);
    await user.type(field('First frame'), '1');
    await user.click(next());
    await user.click(next());
    await user.click(startScan());
    await screen.findByRole('list', { name: 'Background jobs' });
    await expectNoA11yViolations(view.container);
  });
});
