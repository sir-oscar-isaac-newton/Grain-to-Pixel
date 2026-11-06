import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../test-support';
import { ipc, libraryDetails, listeners, makeRoll, resetFake, state } from '../library/fake-ipc';
import { resetLibraryStore, useLibraryStore } from '../library/library-store';
import { useLibraryEvents } from '../library/use-library-events';
import { frameProblems, ScannerSection } from './ScannerSection';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

/** The section as the Import view shows it: its jobs report through the job events. */
function Scanner() {
  useLibraryEvents(true);
  return <ScannerSection />;
}

/** A library with a roll is open. */
function openLibrary(over = {}) {
  state.details = libraryDetails(over);
  const roll = makeRoll({ name: 'Harbour' });
  state.rolls = [roll];
  useLibraryStore.setState({
    phase: 'ready',
    library: state.details,
    rolls: [roll],
    rollId: roll.id,
  });
  return roll;
}

const field = (name: string) => screen.getByRole('spinbutton', { name });
const start = () => screen.getByRole('button', { name: /^Scan( frames?|$)/ });

describe('frameProblems', () => {
  it('wants whole numbers from 1 to 40, the last not before the first', () => {
    expect(frameProblems('1', '40')).toEqual({ first: undefined, last: undefined });
    expect(frameProblems('7', '7')).toEqual({ first: undefined, last: undefined });
    for (const bad of ['', '0', '41', '2.5', '-1', 'x', ' ']) {
      expect(frameProblems(bad, '10').first, bad).toBe('import.scanner.frameRange');
      expect(frameProblems('1', bad).last, bad).toBe('import.scanner.frameRange');
    }
    expect(frameProblems('10', '9').last).toBe('import.scanner.frameOrder');
    expect(frameProblems('x', '9').last).toBeUndefined();
  });
});

describe('ScannerSection', () => {
  beforeEach(() => {
    resetFake();
    resetLibraryStore();
  });

  it('shows the scanner and the settings with their defaults', async () => {
    openLibrary();
    render(<Scanner />);
    expect(await screen.findByText('Scanner: Nikon LS-50 ED.')).toBeInTheDocument();
    expect(field('First frame')).toHaveValue(1);
    expect(field('Last frame')).toHaveValue(40);
    expect(screen.getByRole('combobox', { name: 'Resolution' })).toHaveValue('4000');
    expect(screen.getByRole('combobox', { name: 'Samples per pixel' })).toHaveValue('1');
    expect(screen.getByRole('checkbox', { name: /infrared/ })).toBeChecked();
    expect(screen.getByText('No frame of this roll has been scanned yet.')).toBeInTheDocument();
    expect(start()).toHaveTextContent('Scan frames 1 to 40');
  });

  it('says why there is no scanner, with the hint, and looks again on request', async () => {
    const user = userEvent.setup();
    openLibrary();
    state.devicesFail = 'The scanner has the wrong driver.';
    render(<Scanner />);
    expect(await screen.findByRole('alert')).toHaveTextContent('The scanner has the wrong driver.');
    expect(screen.getByText(/WinUSB driver/)).toBeInTheDocument();
    expect(screen.queryByRole('spinbutton', { name: 'First frame' })).not.toBeInTheDocument();

    state.devicesFail = null;
    await user.click(screen.getByRole('button', { name: 'Look again' }));
    expect(await screen.findByText('Scanner: Nikon LS-50 ED.')).toBeInTheDocument();
    expect(ipc.scannerList).toHaveBeenCalledTimes(2);
  });

  it('refuses frames outside 1 to 40 or in the wrong order, in words', async () => {
    const user = userEvent.setup();
    openLibrary();
    render(<Scanner />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.clear(field('First frame'));
    await user.type(field('First frame'), '0');
    expect(screen.getByText('A frame is a whole number from 1 to 40.')).toBeInTheDocument();
    expect(field('First frame')).toHaveAttribute('aria-invalid', 'true');
    expect(start()).toBeDisabled();

    await user.clear(field('First frame'));
    await user.type(field('First frame'), '12');
    await user.clear(field('Last frame'));
    await user.type(field('Last frame'), '9');
    expect(screen.getByText('The last frame must not come before the first.')).toBeInTheDocument();
    expect(start()).toBeDisabled();

    await user.clear(field('Last frame'));
    await user.type(field('Last frame'), '12');
    expect(start()).toBeEnabled();
    expect(start()).toHaveTextContent('Scan frame 12');
    expect(ipc.scanStart).not.toHaveBeenCalled();
  });

  it('starts the scan with the settings and follows it as a job', async () => {
    const user = userEvent.setup();
    const roll = openLibrary();
    render(<Scanner />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.clear(field('Last frame'));
    await user.type(field('Last frame'), '3');
    await user.selectOptions(screen.getByRole('combobox', { name: 'Resolution' }), '2000');
    await user.selectOptions(screen.getByRole('combobox', { name: 'Samples per pixel' }), '4');
    await user.click(screen.getByRole('checkbox', { name: /infrared/ }));
    await user.click(start());

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
    // While it runs the settings are locked.
    expect(field('First frame')).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Preview the roll' })).toBeDisabled();

    act(() =>
      listeners.progress.forEach((cb) => cb({ id: 'job-scan', stage: 'scanning', fraction: 0.4 })),
    );
    expect(within(jobs).getByRole('progressbar', { name: 'Scan into Harbour' })).toHaveAttribute(
      'value',
      '40',
    );

    // Cancel stops the job.
    await user.click(within(jobs).getByRole('button', { name: /Cancel/ }));
    expect(ipc.jobCancel).toHaveBeenCalledWith('job-scan');
  });

  it('shows the result of a finished scan, and the settings are free again', async () => {
    const user = userEvent.setup();
    openLibrary();
    render(<Scanner />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.click(start());
    await screen.findByRole('list', { name: 'Background jobs' });
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
    expect(await screen.findByText('2 frames scanned')).toBeInTheDocument();
    expect(field('First frame')).toBeEnabled();
    // The roll now has frames 1 and 2: the form offers frame 3.
    expect(await screen.findByText(/Continue at frame 3/)).toBeInTheDocument();
    expect(field('First frame')).toHaveValue(3);
  });

  it('continues at the next frame after a stop, and can start again from frame 1', async () => {
    const user = userEvent.setup();
    const roll = openLibrary();
    state.resume = 6;
    render(<Scanner />);
    expect(await screen.findByText(/Continue at frame 6/)).toBeInTheDocument();
    expect(field('First frame')).toHaveValue(6);
    expect(ipc.scanResumePoint).toHaveBeenCalledWith(roll.id);

    await user.click(screen.getByRole('button', { name: 'Start from frame 1' }));
    expect(field('First frame')).toHaveValue(1);
    await user.click(start());
    await waitFor(() =>
      expect(ipc.scanStart).toHaveBeenCalledWith(
        roll.id,
        expect.objectContaining({ first_frame: 1 }),
      ),
    );
  });

  it('says when every frame of the roll has been scanned', async () => {
    openLibrary();
    state.resume = 41;
    render(<Scanner />);
    expect(
      await screen.findByText('All 40 frames of this roll have been scanned.'),
    ).toBeInTheDocument();
    expect(start()).toBeDisabled();
  });

  it('says why a scan could not start', async () => {
    const user = userEvent.setup();
    openLibrary();
    state.scanFail = 'The scanner is busy with another scan or a roll preview.';
    render(<Scanner />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.click(start());
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The scan could not start: The scanner is busy with another scan or a roll preview.',
    );
    expect(screen.queryByRole('list', { name: 'Background jobs' })).not.toBeInTheDocument();
  });

  it('previews the roll as a strip with its frame count, and can stop the preview', async () => {
    const user = userEvent.setup();
    openLibrary();
    state.previewImage = {
      frame_count: 36,
      pitch: 143,
      strip_width: 5149,
      strip_height: 96,
      strip_jpeg: [0xff, 0xd8, 0xff, 0xd9],
    };
    render(<Scanner />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
    const image = await screen.findByRole('img', { name: /36 frames side by side/ });
    expect(image).toHaveAttribute('src', expect.stringMatching(/^data:image\/jpeg;base64,/));
    expect(image).toHaveAttribute('width', '5149');
    expect(screen.getByText(/36 frames found/)).toBeInTheDocument();
    expect(ipc.scanPreview).toHaveBeenCalledWith('usb:1');
  });

  it('can stop a preview that is running, which is not an error', async () => {
    const user = userEvent.setup();
    openLibrary();
    let stop: (e: unknown) => void = () => undefined;
    ipc.scanPreview.mockImplementationOnce(() => new Promise((_, reject) => (stop = reject)));
    render(<Scanner />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
    await user.click(await screen.findByRole('button', { name: 'Stop the preview' }));
    expect(ipc.scanPreviewCancel).toHaveBeenCalled();
    act(() => stop({ error: { code: 'Cancelled', message: 'The preview was cancelled.' } }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Preview the roll' })).toBeEnabled(),
    );
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('says why a preview failed', async () => {
    const user = userEvent.setup();
    openLibrary();
    state.previewFail = 'The scanner took too long to give a thumbnail of the roll.';
    render(<Scanner />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The roll could not be previewed: The scanner took too long to give a thumbnail of the roll.',
    );
  });

  it('leaves a read-only library alone', async () => {
    openLibrary({ read_only: true });
    render(<Scanner />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    expect(start()).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Preview the roll' })).toBeDisabled();
  });

  it('has no accessibility violations: ready, with a problem, with no scanner, with a preview', async () => {
    const user = userEvent.setup();
    openLibrary();
    const ready = render(<Scanner />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await expectNoA11yViolations(ready.container);
    await user.clear(field('First frame'));
    await expectNoA11yViolations(ready.container);
    await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
    await screen.findByRole('img', { name: /frames side by side/ });
    await expectNoA11yViolations(ready.container);
    ready.unmount();

    state.devicesFail = 'No scanner was found.';
    const none = render(<Scanner />);
    await screen.findByRole('alert');
    await expectNoA11yViolations(none.container);
  });
});
