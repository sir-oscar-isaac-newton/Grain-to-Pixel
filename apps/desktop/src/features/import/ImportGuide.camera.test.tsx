import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import { expectNoA11yViolations } from '../../test-support';
import type { LightStatus } from '../../shared/ipc';
import {
  ipc,
  libraryDetails,
  listeners,
  makeFrames,
  makeRoll,
  resetFake,
  state,
} from '../library/fake-ipc';
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

/** A flat field taken at f/8, which is the aperture of the fake camera. */
const FLAT = { has_flat_field: true, flat_field_f_number: { num: 8, den: 1 } };

/** Levels saved for Portra 400 with the exposure of the fake camera (1/100 s, ISO 100, f/8). */
function saveStockLevels(exposure: object | null = null) {
  state.savedStocks['portra 400'] = {
    levels: [60, 30, 4],
    source: 'stock',
    stock: 'Portra 400',
    exposure: exposure as never,
  };
}
const SAME_EXPOSURE = {
  exposure_time: { num: 1, den: 100 },
  iso: 100,
  f_number: { num: 8, den: 1 },
};

function openLibrary(over: Parameters<typeof makeRoll>[0] = {}) {
  state.details = libraryDetails();
  const roll = makeRoll({ name: 'Harbour', ...over });
  state.rolls = [roll];
  useLibraryStore.setState({ phase: 'ready', library: state.details, rolls: [roll], rollId: null });
  return roll;
}

/** Chooses the roll and the camera on the Roll page, and goes to Set up. */
async function startCamera(user: User) {
  await user.click(await screen.findByRole('radio', { name: 'Harbour' }));
  await user.click(screen.getByRole('radio', { name: 'Camera capture' }));
  await user.click(next());
  await screen.findByRole('heading', { name: 'Set up the camera and light' });
}

async function connect(user: User) {
  await screen.findByText('Camera: Nikon D600');
  await user.click(screen.getByRole('button', { name: 'Connect light' }));
  await screen.findByText('Light connected');
  await screen.findByRole('combobox', { name: 'Aperture' });
}

/** Opens a library with a roll and walks the guide to the named step (every step before it must pass). */
async function begin(user: User, step: string, over: Parameters<typeof makeRoll>[0] = {}) {
  openLibrary(over);
  const view = render(<Guide />);
  await startCamera(user);
  await connect(user);
  for (let i = 0; i < 5 && !current().textContent?.startsWith(step); i += 1) {
    expect(next()).toBeEnabled();
    await user.click(next());
  }
  expect(current()).toHaveTextContent(step);
  return view;
}

const light = (over: Partial<LightStatus> = {}) =>
  act(() => {
    listeners.light.forEach((cb) => cb({ temp_c: 31, vbus_mv: 5000, tripped: false, ...over }));
  });

describe('Import guide: the camera steps', () => {
  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetGuideStore();
    useUiStore.setState({ view: 'import' });
  });

  describe('Set up', () => {
    it('is done once the light is connected and a camera is found, and not before', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<Guide />);
      await startCamera(user);
      expect(current()).toHaveTextContent('Set up');
      expect(next()).toBeDisabled();
      expect(within(current()).queryByText('Done')).not.toBeInTheDocument();
      await connect(user);
      expect(next()).toBeEnabled();
      expect(within(current()).getByText('Done')).toBeInTheDocument();
    });

    it('is not done with a tripped light or without a camera', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<Guide />);
      await startCamera(user);
      await connect(user);
      light({ tripped: true });
      await waitFor(() => expect(next()).toBeDisabled());
      light({ tripped: false });
      await waitFor(() => expect(next()).toBeEnabled());
      state.cameras = [];
      await user.click(screen.getByRole('button', { name: 'Look for camera again' }));
      await screen.findByText('No camera found');
      expect(next()).toBeDisabled();
    });

    it('lists the checks for the rig and keeps nothing of them', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<Guide />);
      await startCamera(user);
      const items = within(screen.getByRole('region', { name: 'Set up the camera and light' }))
        .getAllByRole('listitem')
        .map((li) => li.textContent);
      expect(items).toEqual([
        'The camera is in M mode.',
        'The lens is on manual focus.',
        'The lens ring is at its smallest aperture, where the camera needs it.',
        'The release cable is connected.',
      ]);
      expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
    });

    it('says what exposure the saved levels were set with', async () => {
      const user = userEvent.setup();
      saveStockLevels(SAME_EXPOSURE);
      openLibrary({ film_stock: 'Portra 400' });
      render(<Guide />);
      await startCamera(user);
      expect(
        await screen.findByText(/Levels for Portra 400 were saved at 1\/100 s, ISO 100, f\/8/),
      ).toBeInTheDocument();
    });
  });

  describe('Focus and framing', () => {
    it('has no state: Next is on at once, with live view and the focus check', async () => {
      const user = userEvent.setup();
      await begin(user, 'Focus', FLAT);
      expect(screen.getByRole('heading', { name: 'Focus and framing' })).toHaveFocus();
      expect(screen.getByText(/Put a film frame in the carrier/)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Start live view' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /^Focus check/ })).toBeInTheDocument();
      expect(next()).toBeEnabled();
      expect(within(current()).queryByText('Done')).not.toBeInTheDocument();
    });
  });

  describe('Flat field', () => {
    it('is done when the roll has a flat field taken at the aperture of the camera', async () => {
      const user = userEvent.setup();
      await begin(user, 'Flat field', FLAT);
      expect(within(current()).getByText('Done')).toBeInTheDocument();
      expect(screen.getByText('This roll has a flat field, taken at f/8.')).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Skip the flat field' })).not.toBeInTheDocument();
      expect(next()).toBeEnabled();
    });

    it('is not done without a flat field, and tells to take the film out', async () => {
      const user = userEvent.setup();
      await begin(user, 'Flat field');
      expect(screen.getByText(/Take the film out and touch nothing else/)).toBeInTheDocument();
      expect(within(current()).queryByText('Done')).not.toBeInTheDocument();
      expect(next()).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Capture flat field' })).toBeEnabled();
    });

    it('counts as done when the aperture of either is unknown', async () => {
      const user = userEvent.setup();
      await begin(user, 'Flat field', { has_flat_field: true, flat_field_f_number: null });
      expect(within(current()).getByText('Done')).toBeInTheDocument();
      expect(next()).toBeEnabled();
    });

    it('needs a look when the aperture of the camera is not that of the flat field', async () => {
      const user = userEvent.setup();
      await begin(user, 'Flat field', {
        has_flat_field: true,
        flat_field_f_number: { num: 56, den: 10 },
      });
      expect(
        screen.getByText(
          'The aperture changed since the flat field was taken. Take it again or check the setting.',
        ),
      ).toBeInTheDocument();
      expect(within(current()).getByText('Needs a look')).toBeInTheDocument();
      expect(within(current()).queryByText('Done')).not.toBeInTheDocument();
      expect(next()).toBeDisabled();
    });

    it('needs a look when the aperture is changed after the flat field was taken', async () => {
      const user = userEvent.setup();
      openLibrary(FLAT);
      render(<Guide />);
      await startCamera(user);
      await connect(user);
      await user.selectOptions(screen.getByRole('combobox', { name: 'Aperture' }), '11');
      await user.click(next());
      await user.click(next());
      expect(current()).toHaveTextContent('Flat field');
      expect(within(current()).getByText('Needs a look')).toBeInTheDocument();
      expect(next()).toBeDisabled();
    });

    it('can be skipped for this session: the step bar says skipped, and Next is allowed', async () => {
      const user = userEvent.setup();
      await begin(user, 'Flat field');
      await user.click(screen.getByRole('button', { name: 'Skip the flat field' }));
      expect(current()).toHaveTextContent('Light levels');
      expect(stepButton(/^Flat field/)).toHaveTextContent('Skipped');
      expect(within(stepButton(/^Flat field/)).queryByText('Done')).not.toBeInTheDocument();
      expect(state.settingsPatches.some((p) => JSON.stringify(p).includes('skip'))).toBe(false);

      await user.click(back());
      expect(
        screen.getByText(/Skipped\. Frames will not be corrected for the light's unevenness\./),
      ).toBeInTheDocument();
      expect(next()).toBeEnabled();
    });

    it('takes the flat field through the capture and is done after it', async () => {
      const user = userEvent.setup();
      state.flatFieldFNumber = { num: 8, den: 1 };
      await begin(user, 'Flat field');
      await user.click(screen.getByRole('button', { name: 'Capture flat field' }));
      const dialog = await screen.findByRole('alertdialog');
      await user.click(within(dialog).getByRole('button', { name: 'Capture' }));
      await waitFor(() => expect(ipc.captureFlatField).toHaveBeenCalled());
      await waitFor(() => expect(useLibraryStore.getState().jobs).toHaveLength(1));
      // While it runs, nothing can move on or back.
      expect(next()).toBeDisabled();
      expect(back()).toBeDisabled();
      act(() => {
        listeners.done.forEach((cb) =>
          cb({
            id: state.flatFieldJob,
            result: {
              kind: 'FlatField',
              info: {
                frame_width: 6016,
                frame_height: 4016,
                sources: ['r.NEF', 'g.NEF', 'b.NEF'],
                exposure: { exposure_time: null, iso: null, f_number: { num: 8, den: 1 } },
              },
              levels: [40, 20, 3],
            } as never,
          }),
        );
      });
      await waitFor(() => expect(within(current()).getByText('Done')).toBeInTheDocument());
      expect(next()).toBeEnabled();
    });

    it('copies the flat field of another roll through the dialog', async () => {
      const user = userEvent.setup();
      const other = makeRoll({ name: 'Beach', has_flat_field: true });
      await begin(user, 'Flat field');
      state.rolls = [...state.rolls, other];
      state.flatFields = [
        {
          roll_id: other.id,
          roll_name: 'Beach',
          created_at: '2026-06-01T10:00:00Z',
          exposure: { exposure_time: null, iso: null, f_number: { num: 8, den: 1 } },
        },
      ];
      await user.click(screen.getByRole('button', { name: /Use another roll's flat field/ }));
      const dialog = await screen.findByRole('dialog');
      await user.click(await within(dialog).findByRole('radio', { name: /Beach/ }));
      await user.click(within(dialog).getByRole('button', { name: 'Copy' }));
      await waitFor(() =>
        expect(ipc.flatFieldCopy).toHaveBeenCalledWith(other.id, state.rolls[0]?.id),
      );
      await waitFor(() => expect(within(current()).getByText('Done')).toBeInTheDocument());
    });
  });

  describe('Light levels', () => {
    it('is done by choosing the global levels, and not before', async () => {
      const user = userEvent.setup();
      await begin(user, 'Light levels', FLAT);
      expect(next()).toBeDisabled();
      expect(within(current()).queryByText('Done')).not.toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Use the global levels' }));
      expect(within(current()).getByText('Done')).toBeInTheDocument();
      expect(next()).toBeEnabled();
      expect(
        screen.queryByRole('button', { name: 'Use the global levels' }),
      ).not.toBeInTheDocument();
    });

    it('is not done again once a level is moved', async () => {
      const user = userEvent.setup();
      await begin(user, 'Light levels', FLAT);
      await user.click(screen.getByRole('button', { name: 'Use the global levels' }));
      const red = screen.getByRole('slider', { name: 'Red level' });
      red.focus();
      await user.keyboard('{ArrowRight}');
      expect(within(current()).queryByText('Done')).not.toBeInTheDocument();
      expect(next()).toBeDisabled();
    });

    it('is done when the levels are saved for the film stock', async () => {
      const user = userEvent.setup();
      saveStockLevels();
      await begin(user, 'Light levels', { ...FLAT, film_stock: 'Portra 400' });
      expect(within(current()).getByText('Done')).toBeInTheDocument();
      expect(
        screen.queryByRole('button', { name: 'Use the global levels' }),
      ).not.toBeInTheDocument();
      expect(next()).toBeEnabled();
    });

    it('is done after a calibration, when the camera has the settings it used', async () => {
      const user = userEvent.setup();
      await begin(user, 'Light levels', FLAT);
      await user.click(screen.getByRole('button', { name: /^Calibrate$/ }));
      await waitFor(() => expect(ipc.captureCalibrate).toHaveBeenCalled());
      await waitFor(() => expect(useLibraryStore.getState().jobs).toHaveLength(1));
      act(() => {
        listeners.done.forEach((cb) =>
          cb({
            id: state.calibrationJob,
            result: {
              kind: 'Calibrated',
              levels: [212, 140, 5],
              reached: [0.93, 0.94, 0.987],
              shots: [3, 2, 2],
              warnings: [],
              exposure: {
                exposure_time: { num: 1, den: 100 },
                iso: 100,
                f_number: { num: 8, den: 1 },
              },
            } as never,
          }),
        );
      });
      await waitFor(() => expect(within(current()).getByText('Done')).toBeInTheDocument());
      expect(next()).toBeEnabled();
    });

    it('needs a look after a calibration at settings the camera no longer has', async () => {
      const user = userEvent.setup();
      await begin(user, 'Light levels', FLAT);
      await user.click(screen.getByRole('button', { name: /^Calibrate$/ }));
      await waitFor(() => expect(ipc.captureCalibrate).toHaveBeenCalled());
      await waitFor(() => expect(useLibraryStore.getState().jobs).toHaveLength(1));
      act(() => {
        listeners.done.forEach((cb) =>
          cb({
            id: state.calibrationJob,
            result: {
              kind: 'Calibrated',
              levels: [212, 140, 5],
              reached: [0.93, 0.94, 0.987],
              shots: [3, 2, 2],
              warnings: [],
              // The fake camera is at 1/100 s.
              exposure: {
                exposure_time: { num: 1, den: 50 },
                iso: 100,
                f_number: { num: 8, den: 1 },
              },
            } as never,
          }),
        );
      });
      await screen.findByText(
        'The camera settings changed since these levels were set. Calibrate again or check the settings.',
      );
      expect(within(current()).getByText('Needs a look')).toBeInTheDocument();
      expect(next()).toBeDisabled();
    });

    it.each([
      ['Aperture', '11'],
      ['Shutter speed', '0.0200s'],
      ['ISO', '200'],
    ])(
      'needs a look when the %s differs from the exposure the saved levels were set with',
      async (label, value) => {
        const user = userEvent.setup();
        saveStockLevels(SAME_EXPOSURE);
        openLibrary({ ...FLAT, film_stock: 'Portra 400' });
        render(<Guide />);
        await startCamera(user);
        await connect(user);
        await user.selectOptions(screen.getByRole('combobox', { name: label }), value);
        await user.click(next());
        await user.click(next());
        // The flat field is looked at again only when the aperture changed: skip it then.
        const skip = screen.queryByRole('button', { name: 'Skip the flat field' });
        if (skip) await user.click(skip);
        else await user.click(next());
        expect(current()).toHaveTextContent('Light levels');
        expect(
          screen.getByText(
            'The camera settings changed since these levels were set. Calibrate again or check the settings.',
          ),
        ).toBeInTheDocument();
        expect(within(current()).getByText('Needs a look')).toBeInTheDocument();
        expect(next()).toBeDisabled();
      },
    );
  });

  describe('Capture', () => {
    it('counts the frames of the roll and shows them as a strip, newest last', async () => {
      const user = userEvent.setup();
      saveStockLevels();
      const roll = openLibrary({ ...FLAT, film_stock: 'Portra 400' });
      state.frames = makeFrames(roll.id, 3);
      render(<Guide />);
      await startCamera(user);
      await connect(user);
      for (let i = 0; i < 4; i += 1) await user.click(next());
      expect(current()).toHaveTextContent('Capture');
      expect(await screen.findByText('3 frames captured')).toBeInTheDocument();
      const strip = screen.getByRole('list', { name: 'Frames of this roll' });
      const items = within(strip).getAllByRole('listitem');
      expect(
        items.map((i) => i.textContent || i.querySelector('img')?.getAttribute('alt')),
      ).toEqual([
        expect.stringContaining('Frame 1'),
        expect.stringContaining('Frame 2'),
        expect.stringContaining('Frame 3'),
      ]);
      expect(screen.getByRole('button', { name: 'Finish' })).toBeEnabled();
    });

    it('shows the twelve newest frames of a longer roll, whatever its length', async () => {
      for (const count of [13, 25, 24]) {
        const user = userEvent.setup();
        saveStockLevels();
        const roll = openLibrary({ ...FLAT, film_stock: 'Portra 400' });
        state.frames = makeFrames(roll.id, count);
        const view = render(<Guide />);
        await startCamera(user);
        await connect(user);
        for (let i = 0; i < 4; i += 1) await user.click(next());
        expect(await screen.findByText(`${count} frames captured`)).toBeInTheDocument();
        const items = within(
          screen.getByRole('list', { name: 'Frames of this roll' }),
        ).getAllByRole('listitem');
        const names = items.map(
          (i) => i.textContent || i.querySelector('img')?.getAttribute('alt'),
        );
        expect(names).toHaveLength(12);
        expect(names[0]).toContain(`Frame ${count - 11}`);
        expect(names[11]).toContain(`Frame ${count}`);
        view.unmount();
        resetGuideStore();
      }
    });

    it('holds Back, Next, the step bar and Alt with the arrows while a frame is captured', async () => {
      const user = userEvent.setup();
      saveStockLevels();
      await begin(user, 'Capture', { ...FLAT, film_stock: 'Portra 400' });
      ipc.lightOff.mockClear();
      await user.click(screen.getByRole('button', { name: /^Next frame/ }));
      await waitFor(() => expect(ipc.captureTriplet).toHaveBeenCalled());
      await screen.findByRole('list', { name: 'Steps of this capture' });
      expect(back()).toBeDisabled();
      expect(next()).toBeDisabled();
      for (const button of within(stepBar()).getAllByRole('button')) expect(button).toBeDisabled();
      fireEvent.keyDown(window, { key: 'ArrowLeft', altKey: true });
      fireEvent.keyDown(window, { key: 'ArrowRight', altKey: true });
      expect(current()).toHaveTextContent('Capture');
      expect(useUiStore.getState().view).toBe('import');
      expect(ipc.lightOff).not.toHaveBeenCalled();

      act(() => {
        listeners.done.forEach((cb) =>
          cb({
            id: state.captureJob,
            result: {
              kind: 'Imported',
              frames: ['frame-x'],
              skipped: [],
              frame_ms: [9000],
              warnings: [],
              base_is_guess: false,
              base_replaced: false,
            } as never,
          }),
        );
      });
      await waitFor(() => expect(back()).toBeEnabled());
      expect(screen.getByRole('button', { name: 'Finish' })).toBeEnabled();
    });
  });

  describe('Leaving a step', () => {
    it('stops live view and switches the light off', async () => {
      URL.createObjectURL = vi.fn(() => 'blob:frame');
      URL.revokeObjectURL = vi.fn();
      const user = userEvent.setup();
      await begin(user, 'Focus', FLAT);
      await user.click(screen.getByRole('button', { name: 'Start live view' }));
      await screen.findByRole('img', { name: 'Live view of the camera' });
      ipc.lightOff.mockClear();
      await user.click(next());
      expect(ipc.cameraPreviewStop).toHaveBeenCalled();
      expect(ipc.lightOff).toHaveBeenCalled();
    });

    it('switches the light off when any step is left, going back too', async () => {
      const user = userEvent.setup();
      await begin(user, 'Focus', FLAT);
      ipc.lightOff.mockClear();
      await user.click(back());
      expect(current()).toHaveTextContent('Set up');
      expect(ipc.lightOff).toHaveBeenCalledTimes(1);
    });

    it('keeps the connection and the levels from step to step', async () => {
      const user = userEvent.setup();
      await begin(user, 'Light levels', FLAT);
      await user.click(screen.getByRole('button', { name: 'Use the global levels' }));
      ipc.lightConnect.mockClear();
      ipc.cameraList.mockClear();
      await user.click(stepButton(/^Set up/));
      expect(screen.getByText('Light connected')).toBeInTheDocument();
      await user.click(next());
      await user.click(next());
      await user.click(next());
      expect(within(current()).getByText('Done')).toBeInTheDocument();
      expect(ipc.lightConnect).not.toHaveBeenCalled();
      expect(ipc.cameraList).not.toHaveBeenCalled();
    });

    it('switches the light off when Import is left', async () => {
      const user = userEvent.setup();
      const { unmount } = await begin(user, 'Light levels', FLAT);
      ipc.lightOff.mockClear();
      // Leaving the Import view unmounts the guide.
      unmount();
      expect(ipc.lightOff).toHaveBeenCalledTimes(1);
    });
  });

  it('resumes a roll left at Capture no further than Set up while the light is not connected', async () => {
    const user = userEvent.setup();
    saveStockLevels();
    const roll = openLibrary({ ...FLAT, film_stock: 'Portra 400' });
    state.importGuide[roll.id] = { source: 'camera', step: 'capture' };
    render(<Guide />);
    const choice = await screen.findByRole('radio', { name: /^Harbour/ });
    expect(choice).toHaveAccessibleName(expect.stringContaining('Resume at Capture'));
    await user.click(choice);
    expect(screen.getByRole('radio', { name: 'Camera capture' })).toBeChecked();
    await user.click(next());
    expect(
      await screen.findByRole('heading', { name: 'Set up the camera and light' }),
    ).toBeInTheDocument();
    expect(current()).toHaveTextContent('Set up');
  });

  it('takes one roll through the fakes, from the Roll page to Finish', async () => {
    const user = userEvent.setup();
    state.flatFieldFNumber = { num: 8, den: 1 };
    const roll = openLibrary();
    render(<Guide />);

    // Roll, then the camera.
    await user.click(await screen.findByRole('radio', { name: 'Harbour' }));
    await user.click(screen.getByRole('radio', { name: 'Camera capture' }));
    await user.click(next());

    // Set up: connect.
    await connect(user);
    expect(within(current()).getByText('Done')).toBeInTheDocument();
    await user.click(next());

    // Focus and framing.
    expect(screen.getByRole('heading', { name: 'Focus and framing' })).toBeInTheDocument();
    await user.click(next());

    // Flat field: capture it.
    expect(next()).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Capture flat field' }));
    const dialog = await screen.findByRole('alertdialog');
    await user.click(within(dialog).getByRole('button', { name: 'Capture' }));
    await waitFor(() => expect(useLibraryStore.getState().jobs).toHaveLength(1));
    act(() => {
      listeners.done.forEach((cb) =>
        cb({
          id: state.flatFieldJob,
          result: {
            kind: 'FlatField',
            info: {
              frame_width: 6016,
              frame_height: 4016,
              sources: ['r.NEF', 'g.NEF', 'b.NEF'],
              exposure: { exposure_time: null, iso: null, f_number: { num: 8, den: 1 } },
            },
            levels: [40, 20, 3],
          } as never,
        }),
      );
    });
    await waitFor(() => expect(next()).toBeEnabled());
    await user.click(next());

    // Light levels: the global levels.
    expect(screen.getByRole('heading', { name: 'Set the light levels' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Use the global levels' }));
    await user.click(next());

    // Capture: two frames.
    expect(screen.getByRole('heading', { name: 'Capture the frames' })).toBeInTheDocument();
    expect(await screen.findByText('0 frames captured')).toBeInTheDocument();
    for (const n of [1, 2]) {
      state.captureJob = `job-capture-${n}`;
      await user.click(screen.getByRole('button', { name: /^Next frame/ }));
      await waitFor(() => expect(ipc.captureTriplet).toHaveBeenCalledTimes(n));
      await screen.findByRole('list', { name: 'Steps of this capture' });
      state.frames = makeFrames(roll.id, n);
      act(() => {
        listeners.done.forEach((cb) =>
          cb({
            id: state.captureJob,
            result: {
              kind: 'Imported',
              frames: [`${roll.id}-f0000${n}`],
              skipped: [],
              frame_ms: [9000],
              warnings: [],
              base_is_guess: false,
              base_replaced: false,
            } as never,
          }),
        );
      });
      await screen.findByText(n === 1 ? '1 frame captured' : '2 frames captured');
    }
    const strip = screen.getByRole('list', { name: 'Frames of this roll' });
    expect(within(strip).getAllByRole('listitem')).toHaveLength(2);

    // Finish opens the roll in the Library and leaves the light off.
    ipc.lightOff.mockClear();
    await user.click(screen.getByRole('button', { name: 'Finish' }));
    expect(useUiStore.getState().view).toBe('library');
    expect(useLibraryStore.getState().rollId).toBe(roll.id);
    expect(ipc.lightOff).toHaveBeenCalled();
  });

  it('has no accessibility violations on any of the camera steps', async () => {
    const user = userEvent.setup();
    saveStockLevels(SAME_EXPOSURE);
    openLibrary({ ...FLAT, film_stock: 'Portra 400' });
    const { container } = render(<Guide />);
    await startCamera(user);
    await connect(user);
    await expectNoA11yViolations(container);
    for (const name of [
      'Focus and framing',
      'Take the flat field',
      'Set the light levels',
      'Capture the frames',
    ]) {
      await user.click(next());
      await screen.findByRole('heading', { name });
      await expectNoA11yViolations(container);
    }
  });

  it('has no accessibility violations on a flat field that needs a look, or was skipped', async () => {
    const user = userEvent.setup();
    const { container } = await begin(user, 'Flat field', {
      has_flat_field: true,
      flat_field_f_number: { num: 56, den: 10 },
    });
    await screen.findByRole('alert');
    await expectNoA11yViolations(container);
    await user.click(screen.getByRole('button', { name: 'Skip the flat field' }));
    await user.click(back());
    await screen.findByText(/Skipped\./);
    await expectNoA11yViolations(container);
  });

  it('does not touch the light or the camera while camera capture is off', async () => {
    const roll = openLibrary();
    state.flags = { ...state.flags, guided_capture: false };
    state.importGuide[roll.id] = { source: 'camera', step: 'setup' };
    useGuideStore.getState().setLastRoll(roll.id);
    const { unmount } = render(<Guide />);
    await screen.findByText(/Turn on Guided capture in Settings/);
    await waitFor(() => expect(ipc.settingsGet).toHaveBeenCalled());
    unmount();
    expect(ipc.cameraList).not.toHaveBeenCalled();
    expect(ipc.lightOff).not.toHaveBeenCalled();
  });
});
