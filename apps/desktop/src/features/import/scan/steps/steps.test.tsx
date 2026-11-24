import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../../test-support';
import {
  ipc,
  libraryDetails,
  listeners,
  makeRoll,
  resetFake,
  state,
} from '../../../library/fake-ipc';
import { resetLibraryStore, useLibraryStore } from '../../../library/library-store';
import { ScanSteps } from './test-rig';

vi.mock('../../../../shared/ipc', async () => (await import('../../../library/fake-ipc')).ipc);

/** A library with a roll is open. */
function openLibrary(over = {}, roll: Parameters<typeof makeRoll>[0] = {}) {
  state.details = libraryDetails(over);
  const made = makeRoll({ name: 'Harbour', ...roll });
  state.rolls = [made];
  useLibraryStore.setState({
    phase: 'ready',
    library: state.details,
    rolls: [made],
    rollId: made.id,
  });
  return made;
}

const field = (name: string) => screen.getByRole('spinbutton', { name });
const start = () => screen.getByRole('button', { name: /^Scan( frames?|$)/ });
const ir = () => screen.getByRole('checkbox', { name: /infrared/ });

describe('Scanner steps', () => {
  beforeEach(() => {
    resetFake();
    resetLibraryStore();
  });

  it('shows the scanner and the settings with their defaults', async () => {
    openLibrary();
    render(<ScanSteps />);
    expect(await screen.findByText('Scanner: Nikon LS-50 ED.')).toBeInTheDocument();
    expect(field('First frame')).toHaveValue(1);
    expect(field('Last frame')).toHaveValue(40);
    expect(screen.getByRole('combobox', { name: 'Resolution' })).toHaveValue('4000');
    expect(screen.getByRole('combobox', { name: 'Samples per pixel' })).toHaveValue('1');
    expect(ir()).toBeChecked();
    expect(screen.getByText('No frame of this roll has been scanned yet.')).toBeInTheDocument();
    expect(start()).toHaveTextContent('Scan frames 1 to 40');
  });

  it.each([
    ['bw_silver', false],
    ['color_negative', true],
    ['bw_chromogenic', true],
  ] as const)(
    'starts a %s roll with the infrared %s, and the user can change it',
    async (type, on) => {
      const user = userEvent.setup();
      const roll = openLibrary({}, { film_type: type });
      render(<ScanSteps />);
      await screen.findByText('Scanner: Nikon LS-50 ED.');
      if (on) expect(ir()).toBeChecked();
      else expect(ir()).not.toBeChecked();

      await user.click(ir());
      await user.click(start());
      await waitFor(() =>
        expect(ipc.scanStart).toHaveBeenCalledWith(roll.id, expect.objectContaining({ ir: !on })),
      );
    },
  );

  it('says why there is no scanner, with the hint, and looks again on request', async () => {
    const user = userEvent.setup();
    openLibrary();
    state.devicesFail = 'The scanner has the wrong driver.';
    render(<ScanSteps />);
    expect(await screen.findByRole('alert')).toHaveTextContent('The scanner has the wrong driver.');
    expect(screen.getByText(/WinUSB driver/)).toBeInTheDocument();
    // Without a scanner there is nothing to scan or preview with.
    expect(start()).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Preview the roll' })).toBeDisabled();

    state.devicesFail = null;
    await user.click(screen.getByRole('button', { name: 'Look again' }));
    expect(await screen.findByText('Scanner: Nikon LS-50 ED.')).toBeInTheDocument();
    expect(ipc.scannerList).toHaveBeenCalledTimes(2);
    expect(start()).toBeEnabled();
  });

  it('refuses frames outside 1 to 40 or in the wrong order, in words', async () => {
    const user = userEvent.setup();
    openLibrary();
    render(<ScanSteps />);
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
    render(<ScanSteps />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.clear(field('Last frame'));
    await user.type(field('Last frame'), '3');
    await user.selectOptions(screen.getByRole('combobox', { name: 'Resolution' }), '2000');
    await user.selectOptions(screen.getByRole('combobox', { name: 'Samples per pixel' }), '4');
    await user.click(ir());
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
    expect(screen.getByRole('combobox', { name: 'Resolution' })).toBeDisabled();
    expect(ir()).toBeDisabled();
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
    render(<ScanSteps />);
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
    render(<ScanSteps />);
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
    render(<ScanSteps />);
    expect(
      (await screen.findAllByText('All 40 frames of this roll have been scanned.')).length,
    ).toBe(2);
    expect(start()).toBeDisabled();
  });

  it('says why a scan could not start', async () => {
    const user = userEvent.setup();
    openLibrary();
    state.scanFail = 'The scanner is busy with another scan or a roll preview.';
    render(<ScanSteps />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.click(start());
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The scan could not start: The scanner is busy with another scan or a roll preview.',
    );
    expect(screen.queryByRole('list', { name: 'Background jobs' })).not.toBeInTheDocument();
  });

  it('previews the roll as a strip with its frame count', async () => {
    const user = userEvent.setup();
    openLibrary();
    state.previewImage = {
      frame_count: 36,
      pitch: 143,
      strip_width: 5149,
      strip_height: 96,
      strip_jpeg: [0xff, 0xd8, 0xff, 0xd9],
    };
    render(<ScanSteps />);
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
    render(<ScanSteps />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
    // Frames and Scan both offer the stop while the preview runs.
    const stops = await screen.findAllByRole('button', { name: 'Stop the preview' });
    expect(stops).toHaveLength(2);
    await user.click(stops[0] as HTMLElement);
    expect(ipc.scanPreviewCancel).toHaveBeenCalled();
    act(() => stop({ error: { code: 'Cancelled', message: 'The preview was cancelled.' } }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Preview the roll' })).toBeEnabled(),
    );
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('stops a preview that is still running when the guide leaves the scanner', async () => {
    const user = userEvent.setup();
    openLibrary();
    ipc.scanPreview.mockImplementationOnce(() => new Promise(() => undefined));
    const { unmount } = render(<ScanSteps />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
    await screen.findAllByRole('button', { name: 'Stop the preview' });
    ipc.scanPreviewCancel.mockClear();
    unmount();
    expect(ipc.scanPreviewCancel).toHaveBeenCalledTimes(1);
  });

  it('holds the scan while a preview runs, and starts one scan for a double click', async () => {
    const user = userEvent.setup();
    openLibrary();
    let stop: (e: unknown) => void = () => undefined;
    ipc.scanPreview.mockImplementationOnce(() => new Promise((_, reject) => (stop = reject)));
    render(<ScanSteps />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
    await screen.findAllByRole('button', { name: 'Stop the preview' });
    expect(start()).toBeDisabled();
    act(() => stop({ error: { code: 'Cancelled', message: 'The preview was cancelled.' } }));
    await waitFor(() => expect(start()).toBeEnabled());

    let started: (id: string) => void = () => undefined;
    ipc.scanStart.mockImplementationOnce(() => new Promise((resolve) => (started = resolve)));
    await user.dblClick(start());
    expect(start()).toBeDisabled();
    act(() => started('job-1'));
    await waitFor(() => expect(ipc.scanStart).toHaveBeenCalledTimes(1));
  });

  it('does not stop anything on leaving when no preview runs', async () => {
    openLibrary();
    const { unmount } = render(<ScanSteps />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    ipc.scanPreviewCancel.mockClear();
    unmount();
    expect(ipc.scanPreviewCancel).not.toHaveBeenCalled();
  });

  it('says why a preview failed', async () => {
    const user = userEvent.setup();
    openLibrary();
    state.previewFail = 'The scanner took too long to give a thumbnail of the roll.';
    render(<ScanSteps />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The roll could not be previewed: The scanner took too long to give a thumbnail of the roll.',
    );
  });

  it('leaves a read-only library alone', async () => {
    openLibrary({ read_only: true });
    render(<ScanSteps />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    expect(start()).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Preview the roll' })).toBeDisabled();
  });

  it('has no accessibility violations: ready, with a problem, with no scanner, with a preview', async () => {
    const user = userEvent.setup();
    openLibrary();
    const ready = render(<ScanSteps />);
    await screen.findByText('Scanner: Nikon LS-50 ED.');
    await expectNoA11yViolations(ready.container);
    await user.clear(field('First frame'));
    await expectNoA11yViolations(ready.container);
    await user.click(screen.getByRole('button', { name: 'Preview the roll' }));
    await screen.findByRole('img', { name: /frames side by side/ });
    await expectNoA11yViolations(ready.container);
    ready.unmount();

    state.devicesFail = 'No scanner was found.';
    const none = render(<ScanSteps />);
    await screen.findByRole('alert');
    await expectNoA11yViolations(none.container);
  });
});
