import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../test-support';
import { IpcError, type ImportWarning, type LightStatus } from '../../shared/ipc';
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
import { CaptureSection } from './CaptureSection';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

/** The section as the Import view shows it: its job reports through the job events. */
function Capture() {
  useLibraryEvents(true);
  return <CaptureSection />;
}

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

const reading = (over: Partial<LightStatus> = {}): LightStatus => ({
  temp_c: 31,
  vbus_mv: 5000,
  tripped: false,
  ...over,
});

const emitLight = (status: LightStatus) =>
  act(() => {
    listeners.light.forEach((cb) => cb(status));
  });

/** Renders the section with a light that answers and a camera that is there. */
async function ready(user = userEvent.setup()) {
  const roll = openLibrary();
  const view = render(<Capture />);
  await screen.findByText('Camera: Nikon D600');
  await user.click(screen.getByRole('button', { name: 'Connect light' }));
  await screen.findByText('Light connected');
  return { roll, user, ...view };
}

const next = () => screen.getByRole('button', { name: /^(Next frame|Capturing)/ });

/** Starts a capture and returns the job id the fake gave it. */
async function startCapture(user: ReturnType<typeof userEvent.setup>) {
  await user.click(next());
  await waitFor(() => expect(ipc.captureTriplet).toHaveBeenCalled());
  await screen.findByRole('list', { name: 'Steps of this capture' });
  return state.captureJob;
}

const finish = (id: string, frame: string, warnings: ImportWarning[] = []) =>
  act(() => {
    listeners.done.forEach((cb) =>
      cb({
        id,
        result: {
          kind: 'Imported',
          frames: [frame],
          skipped: [],
          frame_ms: [9000],
          warnings,
          base_is_guess: false,
          base_replaced: false,
        } as never,
      }),
    );
  });

const calibrateButton = () => screen.getByRole('button', { name: /^Calibrat(e|ing)/ });

const CALIBRATED = {
  kind: 'Calibrated',
  levels: [212, 140, 5],
  reached: [0.93, 0.94, 0.987],
  shots: [3, 2, 2],
  warnings: [{ kind: 'coarse_level', channel: 'blue', level: 5 }],
  exposure: { exposure_time: { num: 1, den: 50 }, iso: 100, f_number: { num: 8, den: 1 } },
};

/** Starts a calibration and returns the job id the fake gave it. */
async function startCalibration(user: ReturnType<typeof userEvent.setup>) {
  await user.click(calibrateButton());
  await waitFor(() => expect(ipc.captureCalibrate).toHaveBeenCalled());
  await waitFor(() => expect(useLibraryStore.getState().jobs).toHaveLength(1));
  return state.calibrationJob;
}

const calibrationProgress = (id: string, stage: string, fraction: number) =>
  act(() => listeners.progress.forEach((cb) => cb({ id, stage, fraction })));

const calibrationDone = (id: string, result: object = CALIBRATED) =>
  act(() => listeners.done.forEach((cb) => cb({ id, result: result as never })));

const calibrationError = (id: string, code: string, message: string) =>
  act(() => listeners.error.forEach((cb) => cb({ id, error: { code, message } as never })));

describe('CaptureSection', () => {
  beforeEach(() => {
    resetFake();
    resetLibraryStore();
  });

  describe('status', () => {
    it('shows the light as not connected until it is connected', async () => {
      openLibrary();
      render(<Capture />);
      expect(await screen.findByText('Light not connected')).toBeInTheDocument();
      expect(next()).toBeDisabled();
    });

    it('connects the light by probing the ports and shows it connected', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<Capture />);
      await screen.findByText('Camera: Nikon D600');
      await user.click(screen.getByRole('button', { name: 'Connect light' }));
      expect(await screen.findByText('Light connected')).toBeInTheDocument();
      expect(ipc.lightConnect).toHaveBeenCalledWith(null);
      expect(screen.getByRole('button', { name: 'Reconnect light' })).toBeInTheDocument();
      expect(next()).toBeEnabled();
    });

    it('shows the message when the light cannot be connected', async () => {
      const user = userEvent.setup();
      state.lightFail = 'The light did not answer.';
      openLibrary();
      render(<Capture />);
      await user.click(await screen.findByRole('button', { name: 'Connect light' }));
      expect(await screen.findByRole('alert')).toHaveTextContent('The light did not answer.');
      expect(screen.getByText('Light not connected')).toBeInTheDocument();
    });

    it('shows the temperature from the light readings, and warns when it is warm', async () => {
      await ready();
      emitLight(reading({ temp_c: 31.4 }));
      expect(await screen.findByText('Light connected, 31 °C')).toBeInTheDocument();
      emitLight(reading({ temp_c: 52 }));
      expect(await screen.findByText('Light warm, 52 °C')).toBeInTheDocument();
      expect(screen.getByText(/getting warm/)).toBeInTheDocument();
    });

    it('warns about a low supply voltage', async () => {
      await ready();
      emitLight(reading({ vbus_mv: 4600 }));
      expect(await screen.findByText(/supply of the light is low \(4600 mV\)/)).toBeInTheDocument();
    });

    it('shows a tripped light as an error, offers Reconnect and blocks capturing', async () => {
      const user = userEvent.setup();
      await ready(user);
      emitLight(reading({ temp_c: 70, tripped: true }));
      expect(await screen.findByText('Light too hot and switched off')).toBeInTheDocument();
      expect(next()).toBeDisabled();
      await user.click(screen.getByRole('button', { name: 'Reconnect light' }));
      expect(ipc.lightConnect).toHaveBeenCalledTimes(2);
      await waitFor(() => expect(next()).toBeEnabled());
      expect(screen.queryByText('Light too hot and switched off')).not.toBeInTheDocument();
    });

    it('names the camera, or says none was found and looks again on request', async () => {
      const user = userEvent.setup();
      state.cameras = [];
      openLibrary();
      render(<Capture />);
      expect(await screen.findByText('No camera found')).toBeInTheDocument();
      state.cameras = [{ id: 'cam:1', name: 'Nikon D600' }];
      await user.click(screen.getByRole('button', { name: 'Look for camera again' }));
      expect(await screen.findByText('Camera: Nikon D600')).toBeInTheDocument();
    });
  });

  describe('levels', () => {
    const slider = (name: string) => screen.getByRole('slider', { name });

    it('starts from the levels of the chosen roll and shows where they come from', async () => {
      openLibrary();
      render(<Capture />);
      const red = await screen.findByRole('slider', { name: 'Red level' });
      expect(red).toHaveAttribute('aria-valuenow', '44');
      expect(slider('Blue level')).toHaveAttribute('aria-valuenow', '2');
      expect(screen.getByText('Default levels')).toBeInTheDocument();
      expect(ipc.captureLevelsGet).toHaveBeenCalledWith(state.rolls[0]?.id);
    });

    it('uses the levels saved for the film stock of the roll, and the default for another', async () => {
      const user = userEvent.setup();
      const portra = makeRoll({ name: 'Portra roll', film_stock: ' Portra 400 ' });
      const plain = makeRoll({ name: 'Plain roll' });
      state.savedStocks['portra 400'] = {
        levels: [60, 30, 4],
        source: 'stock',
        stock: 'Portra 400',
        exposure: null,
      };
      state.rolls = [portra, plain];
      state.details = libraryDetails();
      useLibraryStore.setState({
        phase: 'ready',
        library: state.details,
        rolls: [portra, plain],
        rollId: portra.id,
      });
      render(<Capture />);
      const red = await screen.findByRole('slider', { name: 'Red level' });
      await waitFor(() => expect(red).toHaveAttribute('aria-valuenow', '60'));
      expect(screen.getByText('Levels saved for Portra 400')).toBeInTheDocument();

      await user.selectOptions(
        screen.getByRole('combobox', { name: 'Import into roll' }),
        plain.id,
      );
      await waitFor(() => expect(red).toHaveAttribute('aria-valuenow', '44'));
      expect(screen.getByText('Default levels')).toBeInTheDocument();
      expect(ipc.captureLevelsGet).toHaveBeenLastCalledWith(plain.id);
    });

    it('changes only the working levels: no call is made and nothing is saved', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<Capture />);
      const red = await screen.findByRole('slider', { name: 'Red level' });
      red.focus();
      await user.keyboard('{ArrowRight}');
      expect(red).toHaveAttribute('aria-valuenow', '45');
      expect(screen.getByText('Changed, not saved')).toBeInTheDocument();
      expect(ipc.captureProfileSet).not.toHaveBeenCalled();
      expect(ipc.captureLevelsSave).not.toHaveBeenCalled();
      // Moving a slider never lights the light.
      expect(ipc.lightSet).not.toHaveBeenCalled();
    });

    it('shows why the levels could not be read', async () => {
      state.levelsFail = 'The settings could not be read.';
      openLibrary();
      render(<Capture />);
      expect(await screen.findByText('The settings could not be read.')).toBeInTheDocument();
    });
  });

  describe('saving levels', () => {
    it('is not possible while a capture runs', async () => {
      const { user } = await ready();
      await screen.findByRole('slider', { name: 'Red level' });
      await user.click(screen.getByRole('button', { name: 'Test shot, red' }));
      await waitFor(() =>
        expect(screen.getByRole('button', { name: 'Save as default' })).toBeEnabled(),
      );
      await startCapture(user);
      expect(screen.getByRole('button', { name: 'Save as default' })).toBeDisabled();
      expect(screen.getByRole('slider', { name: 'Red level' })).toHaveAttribute('data-disabled');
    });

    const saveDefault = () => screen.getByRole('button', { name: 'Save as default' });

    it('needs a test shot at the current levels, and a moved slider needs a new one', async () => {
      const { user } = await ready();
      await screen.findByRole('slider', { name: 'Red level' });
      expect(saveDefault()).toBeDisabled();
      expect(
        screen.getByText('Take a test shot at these levels to save them.'),
      ).toBeInTheDocument();

      await user.click(screen.getByRole('button', { name: 'Test shot, red' }));
      await waitFor(() => expect(saveDefault()).toBeEnabled());

      screen.getByRole('slider', { name: 'Red level' }).focus();
      await user.keyboard('{ArrowRight}');
      expect(saveDefault()).toBeDisabled();
      await user.click(screen.getByRole('button', { name: 'Test shot, red' }));
      await waitFor(() => expect(saveDefault()).toBeEnabled());
    });

    it('offers Save for the film stock only when the roll has one', async () => {
      const { user } = await ready();
      await screen.findByRole('slider', { name: 'Red level' });
      expect(screen.queryByRole('button', { name: /^Save for/ })).not.toBeInTheDocument();
      useLibraryStore.setState({ rolls: [makeRoll({ name: 'Harbour', film_stock: ' HP5 ' })] });
      const rolls = useLibraryStore.getState().rolls;
      state.rolls = rolls;
      useLibraryStore.setState({ rollId: rolls[0]?.id ?? null });
      expect(await screen.findByRole('button', { name: 'Save for HP5' })).toBeDisabled();
      await user.click(screen.getByRole('button', { name: 'Test shot, green' }));
      await waitFor(() =>
        expect(screen.getByRole('button', { name: 'Save for HP5' })).toBeEnabled(),
      );
    });

    it('saves as the default with the levels and the camera settings of the test shot', async () => {
      const { user } = await ready();
      const red = await screen.findByRole('slider', { name: 'Red level' });
      red.focus();
      await user.keyboard('{ArrowRight}{ArrowRight}');
      await user.click(screen.getByRole('button', { name: 'Test shot, red' }));
      await waitFor(() => expect(saveDefault()).toBeEnabled());
      await user.click(saveDefault());
      await waitFor(() =>
        expect(ipc.captureLevelsSave).toHaveBeenCalledWith(
          'default',
          [46, 22, 2],
          state.testShot.exposure,
        ),
      );
      expect(await screen.findByText('Levels saved.')).toBeInTheDocument();
      expect(screen.getByText('Default levels')).toBeInTheDocument();
      expect(ipc.captureLevelsGet).toHaveBeenCalledTimes(2);
    });

    it('saves for the film stock and then shows the levels as saved for it', async () => {
      const user = userEvent.setup();
      const roll = makeRoll({ name: 'Harbour', film_stock: 'Portra 400' });
      state.details = libraryDetails();
      state.rolls = [roll];
      useLibraryStore.setState({
        phase: 'ready',
        library: state.details,
        rolls: [roll],
        rollId: roll.id,
      });
      render(<Capture />);
      await screen.findByText('Camera: Nikon D600');
      await user.click(screen.getByRole('button', { name: 'Connect light' }));
      await screen.findByText('Light connected');
      const red = await screen.findByRole('slider', { name: 'Red level' });
      red.focus();
      await user.keyboard('{ArrowRight}');
      expect(screen.getByText('Changed, not saved')).toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Test shot, red' }));
      const save = await screen.findByRole('button', { name: 'Save for Portra 400' });
      await waitFor(() => expect(save).toBeEnabled());
      await user.click(save);
      await waitFor(() =>
        expect(ipc.captureLevelsSave).toHaveBeenCalledWith(
          { stock: 'Portra 400' },
          [45, 22, 2],
          state.testShot.exposure,
        ),
      );
      expect(await screen.findByText('Levels saved for Portra 400')).toBeInTheDocument();
    });

    it('says when the test shot camera settings differ from those the levels were saved with', async () => {
      state.savedDefaultExposure = {
        exposure_time: { num: 1, den: 125 },
        iso: 200,
        f_number: null,
      };
      const { user } = await ready();
      await screen.findByRole('slider', { name: 'Red level' });
      expect(
        screen.queryByText('The camera settings differ from when these levels were saved.'),
      ).not.toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Test shot, red' }));
      expect(
        await screen.findByText('The camera settings differ from when these levels were saved.'),
      ).toBeInTheDocument();
    });

    it('shows why a save was refused', async () => {
      const { user } = await ready();
      await screen.findByRole('slider', { name: 'Red level' });
      await user.click(screen.getByRole('button', { name: 'Test shot, red' }));
      await waitFor(() => expect(saveDefault()).toBeEnabled());
      state.levelsFail = 'The settings could not be written.';
      await user.click(saveDefault());
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The settings could not be written.',
      );
    });

    it('is not blocked by a read-only library', async () => {
      openLibrary({ read_only: true });
      const user = userEvent.setup();
      render(<Capture />);
      await screen.findByText('Camera: Nikon D600');
      await user.click(screen.getByRole('button', { name: 'Connect light' }));
      await screen.findByText('Light connected');
      await screen.findByRole('slider', { name: 'Red level' });
      await user.click(screen.getByRole('button', { name: 'Test shot, red' }));
      await waitFor(() => expect(saveDefault()).toBeEnabled());
      expect(screen.getByRole('slider', { name: 'Red level' })).not.toHaveAttribute(
        'data-disabled',
      );
    });
  });

  describe('test shot', () => {
    const shotOf = (clipped: number | null) => {
      const histogram = new Array<number>(256).fill(0);
      histogram[100] = 0.5;
      state.testShot = {
        ...state.testShot,
        histogram,
        clipped_pct: clipped,
      } as typeof state.testShot;
    };

    it('shows the histogram of the colour and how much is clipped, without a warning', async () => {
      shotOf(0.1);
      const { user } = await ready();
      await user.click(screen.getByRole('button', { name: 'Test shot, green' }));
      expect(
        await screen.findByRole('img', { name: 'Histogram of the green test shot' }),
      ).toBeInTheDocument();
      expect(ipc.captureTest).toHaveBeenCalledWith('green', [44, 22, 2]);
      expect(screen.getByText('0.10 % clipped')).toBeInTheDocument();
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    });

    it('warns when more than 0.1 % is clipped', async () => {
      shotOf(0.4);
      const { user } = await ready();
      await user.click(screen.getByRole('button', { name: 'Test shot, blue' }));
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The blue test shot is clipped (0.40 %)',
      );
    });

    it('shows the message when the test shot fails, and is off without a camera', async () => {
      state.testFail = 'The camera did not answer.';
      const { user } = await ready();
      await user.click(screen.getByRole('button', { name: 'Test shot, red' }));
      expect(await screen.findByText('The camera did not answer.')).toBeInTheDocument();
    });

    it('cannot be used before the light is connected', async () => {
      openLibrary();
      render(<Capture />);
      await screen.findByText('Camera: Nikon D600');
      expect(screen.getByRole('button', { name: 'Test shot, red' })).toBeDisabled();
    });
  });

  describe('next frame', () => {
    it('captures into the roll, follows the steps and shows the frame, the counter and warnings', async () => {
      const { roll, user } = await ready();
      state.frames = makeFrames(roll.id, 7);
      const id = await startCapture(user);
      expect(ipc.captureTriplet).toHaveBeenCalledWith(roll.id, [44, 22, 2]);
      expect(useLibraryStore.getState().jobs[0]).toMatchObject({ id, kind: 'capture' });

      act(() => listeners.step.forEach((cb) => cb({ channel: 'red', state: 'lit' })));
      expect(await screen.findByText('red: light on')).toBeInTheDocument();
      act(() => listeners.step.forEach((cb) => cb({ channel: 'red', state: 'received' })));
      act(() => listeners.step.forEach((cb) => cb({ channel: 'green', state: 'fired' })));
      expect(await screen.findByText('red: picture received')).toBeInTheDocument();
      expect(screen.getByText('green: picture taken')).toBeInTheDocument();
      expect(screen.getByText('blue: waiting')).toBeInTheDocument();
      expect(next()).toBeDisabled();

      finish(id, state.frames[6]?.id ?? 'f', [
        { frame_id: 'f', kind: { kind: 'high_temperature', temp_c: 47 } },
        { frame_id: 'f', kind: { kind: 'low_supply', vbus_mv: 4600 } },
        { frame_id: 'f', kind: { kind: 'iso_mismatch' } },
        { frame_id: 'f', kind: { kind: 'exposure_time_mismatch' } },
        { frame_id: 'f', kind: { kind: 'clipped', channel: 'blue', per_mille: 12 } },
        { frame_id: 'f', kind: { kind: 'exposure_differs_from_levels' } },
      ]);
      expect(await screen.findByText('Frame 7 on this roll')).toBeInTheDocument();
      expect(screen.getByText('The light was warm during this frame (47 °C).')).toBeInTheDocument();
      expect(screen.getByText(/dropped to 4600 mV/)).toBeInTheDocument();
      expect(screen.getByText(/different ISO values/)).toBeInTheDocument();
      expect(screen.getByText(/different shutter times/)).toBeInTheDocument();
      expect(
        screen.getByText('The blue picture is clipped (1.2 %). Lower its level.'),
      ).toBeInTheDocument();
      expect(
        screen.getByText('The camera settings differ from when these levels were saved.'),
      ).toBeInTheDocument();
      expect(screen.queryByRole('list', { name: 'Steps of this capture' })).not.toBeInTheDocument();
      expect(next()).toBeEnabled();
    });

    it('captures with the working levels, saved or changed', async () => {
      const { roll, user } = await ready();
      const red = await screen.findByRole('slider', { name: 'Red level' });
      red.focus();
      await user.keyboard('{ArrowRight}{ArrowRight}{ArrowRight}');
      await user.click(next());
      await waitFor(() => expect(ipc.captureTriplet).toHaveBeenCalledWith(roll.id, [47, 22, 2]));
    });

    it('starts with the Space key', async () => {
      const { roll, user } = await ready();
      // Clicking Connect left the focus on that button, which keeps its own Space.
      (document.activeElement as HTMLElement | null)?.blur();
      await user.keyboard(' ');
      await waitFor(() => expect(ipc.captureTriplet).toHaveBeenCalledWith(roll.id, [44, 22, 2]));
    });

    it('ignores Space while a field, a slider or a select has the focus', async () => {
      const { user } = await ready();
      const slider = screen.getByRole('slider', { name: 'Red level' });
      slider.focus();
      await user.keyboard(' ');
      screen.getByRole('combobox', { name: 'Import into roll' }).focus();
      await user.keyboard(' ');
      const input = document.createElement('input');
      document.body.append(input);
      input.focus();
      await user.keyboard(' ');
      input.remove();
      expect(ipc.captureTriplet).not.toHaveBeenCalled();
    });

    it('ignores Space while a capture runs', async () => {
      const { user } = await ready();
      await startCapture(user);
      (document.activeElement as HTMLElement | null)?.blur();
      await user.keyboard(' ');
      await user.keyboard(' ');
      expect(ipc.captureTriplet).toHaveBeenCalledTimes(1);
    });

    it('cancels the running capture', async () => {
      const { user } = await ready();
      const id = await startCapture(user);
      await user.click(screen.getByRole('button', { name: 'Cancel' }));
      expect(ipc.jobCancel).toHaveBeenCalledWith(id);
      act(() =>
        listeners.error.forEach((cb) =>
          cb({ id, error: { code: 'Cancelled', message: 'Cancelled' } as never }),
        ),
      );
      expect(await screen.findByText('The capture was cancelled.')).toBeInTheDocument();
      expect(next()).toBeEnabled();
    });

    it('shows the message when the capture fails', async () => {
      const { user } = await ready();
      const id = await startCapture(user);
      act(() =>
        listeners.error.forEach((cb) =>
          cb({ id, error: { code: 'CaptureFailed', message: 'The camera timed out.' } as never }),
        ),
      );
      expect(await screen.findByRole('alert')).toHaveTextContent('The camera timed out.');
    });

    it('shows the message when the capture cannot start', async () => {
      const { user } = await ready();
      state.captureFail = 'The light is too hot.';
      await user.click(next());
      expect(await screen.findByRole('alert')).toHaveTextContent('The light is too hot.');
      expect(useLibraryStore.getState().jobs).toEqual([]);
    });

    it('cannot capture into a read-only library', async () => {
      openLibrary({ read_only: true });
      const user = userEvent.setup();
      render(<Capture />);
      await screen.findByText('Camera: Nikon D600');
      await user.click(screen.getByRole('button', { name: 'Connect light' }));
      await screen.findByText('Light connected');
      expect(next()).toBeDisabled();
    });
  });

  describe('camera settings', () => {
    const select = (name: string) => screen.getByRole('combobox', { name });
    const absent = (kind: 'iso' | 'aperture') =>
      state.cameraSettings.map((s) =>
        s.kind === kind ? { kind, value: null, choices: [], writable: false } : s,
      );

    it('shows ISO, shutter speed and aperture with the choices the camera allows', async () => {
      await ready();
      const iso = await screen.findByRole('combobox', { name: 'ISO' });
      expect(iso).toHaveValue('100');
      expect(select('Shutter speed')).toHaveValue('0.0100s');
      expect(select('Aperture')).toHaveValue('8');
      const options = (el: HTMLElement) =>
        Array.from(el.querySelectorAll('option')).map((o) => o.textContent);
      expect(options(iso)).toEqual(['100', '200', '400', '800']);
      expect(options(select('Aperture'))).toEqual(['5.6', '8', '11']);
      expect(ipc.cameraSettingsGet).toHaveBeenCalled();
    });

    it('shows shutter speeds as fractions and sends the raw choice of the camera', async () => {
      const { user } = await ready();
      const shutter = await screen.findByRole('combobox', { name: 'Shutter speed' });
      expect(Array.from(shutter.querySelectorAll('option')).map((o) => o.textContent)).toEqual([
        '1/250 s',
        '1/100 s',
        '1/60 s',
        '1/50 s',
      ]);
      expect(screen.getByRole('option', { name: '1/50 s' })).toHaveValue('0.0200s');
      await user.selectOptions(shutter, '0.0200s');
      expect(ipc.cameraSettingsSet).toHaveBeenCalledWith('shutter_speed', '0.0200s');
    });

    it('sets a change on the camera and shows what the camera answers', async () => {
      const { user } = await ready();
      await screen.findByRole('combobox', { name: 'ISO' });
      await user.selectOptions(select('ISO'), '400');
      expect(ipc.cameraSettingsSet).toHaveBeenCalledWith('iso', '400');
      await waitFor(() => expect(select('ISO')).toHaveValue('400'));
      expect(select('ISO')).toBeEnabled();
    });

    it('shows the reply of the camera, which may differ from what was asked', async () => {
      const { user } = await ready();
      await screen.findByRole('combobox', { name: 'ISO' });
      ipc.cameraSettingsSet.mockImplementationOnce(async () => {
        const settings = state.cameraSettings.map((s) => ({ ...s }));
        const shutter = settings.find((s) => s.kind === 'shutter_speed');
        if (shutter) shutter.value = '0.0040s';
        return settings;
      });
      await user.selectOptions(select('Aperture'), '11');
      await waitFor(() => expect(select('Shutter speed')).toHaveValue('0.0040s'));
    });

    it('says the aperture is set on the lens when the camera has none to set', async () => {
      state.cameraSettings = absent('aperture');
      await ready();
      expect(await screen.findByText('Set on the lens')).toBeInTheDocument();
      expect(screen.queryByRole('combobox', { name: 'Aperture' })).not.toBeInTheDocument();
      expect(select('ISO')).toBeEnabled();
    });

    it('says Not available for an ISO the camera does not have', async () => {
      state.cameraSettings = absent('iso');
      await ready();
      expect(await screen.findByText('Not available')).toBeInTheDocument();
      expect(screen.queryByRole('combobox', { name: 'ISO' })).not.toBeInTheDocument();
    });

    it('greys a setting the camera does not let change, but still shows it', async () => {
      state.cameraSettings = state.cameraSettings.map((s) =>
        s.kind === 'shutter_speed' ? { ...s, writable: false } : s,
      );
      await ready();
      await screen.findByRole('combobox', { name: 'ISO' });
      expect(select('Shutter speed')).toBeDisabled();
      expect(select('Shutter speed')).toHaveValue('0.0100s');
      expect(select('ISO')).toBeEnabled();
    });

    it('cannot be used while a capture runs, and reads the camera again afterwards', async () => {
      const { user } = await ready();
      await screen.findByRole('combobox', { name: 'ISO' });
      await screen.findByRole('slider', { name: 'Red level' });
      const id = await startCapture(user);
      for (const name of ['ISO', 'Shutter speed', 'Aperture']) {
        expect(select(name)).toBeDisabled();
      }
      ipc.cameraSettingsGet.mockClear();
      finish(id, 'frame-1');
      await waitFor(() => expect(ipc.cameraSettingsGet).toHaveBeenCalled());
      await waitFor(() => expect(select('ISO')).toBeEnabled());
    });

    it('is off while a change is on its way, and so are the test shot and Next frame', async () => {
      const { user } = await ready();
      await screen.findByRole('combobox', { name: 'ISO' });
      let release: () => void = () => undefined;
      ipc.cameraSettingsSet.mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            release = () => resolve(state.cameraSettings.map((s) => ({ ...s })));
          }),
      );
      await user.selectOptions(select('ISO'), '200');
      expect(select('Shutter speed')).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Test shot, red' })).toBeDisabled();
      expect(next()).toBeDisabled();
      act(() => release());
      await waitFor(() => expect(select('Shutter speed')).toBeEnabled());
      expect(screen.getByRole('button', { name: 'Test shot, red' })).toBeEnabled();
    });

    it('shows why the settings could not be read or a change was refused', async () => {
      state.cameraSettingsFail = 'The camera is busy. Wait a moment and try again.';
      const { user } = await ready();
      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('The camera is busy. Wait a moment and try again.');
      expect(screen.queryByRole('combobox', { name: 'ISO' })).not.toBeInTheDocument();

      state.cameraSettingsFail = null;
      await user.click(screen.getByRole('button', { name: 'Look for camera again' }));
      await screen.findByRole('combobox', { name: 'ISO' });
      state.cameraSettingsSetFail = 'The camera does not let this setting be changed now.';
      await user.selectOptions(select('ISO'), '800');
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The camera does not let this setting be changed now.',
      );
      expect(select('ISO')).toHaveValue('100');
    });

    it('shows nothing and asks nothing without a camera', async () => {
      state.cameras = [];
      openLibrary();
      render(<Capture />);
      await screen.findByText('No camera found');
      expect(ipc.cameraSettingsGet).not.toHaveBeenCalled();
      expect(screen.queryByText('Camera settings')).not.toBeInTheDocument();
    });

    it('says what exposure the default levels were saved at', async () => {
      state.savedDefaultExposure = {
        exposure_time: { num: 1, den: 125 },
        iso: 200,
        f_number: { num: 56, den: 10 },
      };
      await ready();
      expect(
        await screen.findByText('Default levels were saved at 1/125 s, ISO 200, f/5.6'),
      ).toBeInTheDocument();
    });

    it('names the film stock the levels were saved for', async () => {
      const portra = makeRoll({ name: 'Portra roll', film_stock: 'Portra 400' });
      state.savedStocks['portra 400'] = {
        levels: [60, 30, 4],
        source: 'stock',
        stock: 'Portra 400',
        exposure: { exposure_time: { num: 2, den: 1 }, iso: 100, f_number: { num: 8, den: 1 } },
      };
      state.rolls = [portra];
      state.details = libraryDetails();
      useLibraryStore.setState({
        phase: 'ready',
        library: state.details,
        rolls: [portra],
        rollId: portra.id,
      });
      render(<Capture />);
      expect(
        await screen.findByText('Levels for Portra 400 were saved at 2 s, ISO 100, f/8'),
      ).toBeInTheDocument();
    });

    it('says nothing about an exposure when none was saved', async () => {
      openLibrary();
      render(<Capture />);
      await screen.findByRole('slider', { name: 'Red level' });
      expect(screen.queryByText(/were saved at/)).not.toBeInTheDocument();
    });
  });

  describe('calibration', () => {
    it('starts with the working levels and the whole frame, and tracks the job', async () => {
      const { roll, user } = await ready();
      expect(calibrateButton()).toBeEnabled();
      expect(screen.getByText(/Frame clear film base/)).toBeInTheDocument();
      const id = await startCalibration(user);
      expect(ipc.captureCalibrate).toHaveBeenCalledWith(roll.id, [44, 22, 2]);
      expect(useLibraryStore.getState().jobs[0]).toMatchObject({ id, kind: 'calibration' });
    });

    it('needs the light and the camera', async () => {
      openLibrary();
      render(<Capture />);
      await screen.findByText('Camera: Nikon D600');
      expect(calibrateButton()).toBeDisabled();
    });

    it('shows the progress and Cancel, and holds the other controls while it runs', async () => {
      const { user } = await ready();
      const id = await startCalibration(user);
      calibrationProgress(id, 'Red: shot 2, level 120', 2 / 18);
      expect(await screen.findByText('Red: shot 2, level 120')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Cancel' })).toBeEnabled();
      expect(calibrateButton()).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Test shot, red' })).toBeDisabled();
      expect(next()).toBeDisabled();
      expect(screen.getByRole('slider', { name: 'Red level' })).toHaveAttribute('data-disabled');
      expect(screen.getByRole('combobox', { name: 'ISO' })).toBeDisabled();
      await user.click(screen.getByRole('button', { name: 'Cancel' }));
      expect(ipc.jobCancel).toHaveBeenCalledWith(id);
    });

    it('sets the sliders, lists what it found and allows saving without another test shot', async () => {
      const { user } = await ready();
      expect(screen.getByRole('button', { name: 'Save as default' })).toBeDisabled();
      const id = await startCalibration(user);
      calibrationDone(id);
      expect(await screen.findByText('Red 212: 93 % of full scale in 3 shots')).toBeInTheDocument();
      expect(screen.getByText('Green 140: 94 % of full scale in 2 shots')).toBeInTheDocument();
      expect(screen.getByText('Blue 5: 99 % of full scale in 2 shots')).toBeInTheDocument();
      expect(
        screen.getByText(/Blue ended at level 5, where one step is a big change/),
      ).toBeInTheDocument();
      expect(screen.getByRole('slider', { name: 'Red level' })).toHaveValue(212);
      expect(screen.getByRole('slider', { name: 'Green level' })).toHaveValue(140);
      expect(screen.getByRole('slider', { name: 'Blue level' })).toHaveValue(5);
      expect(screen.getByText('Changed, not saved')).toBeInTheDocument();
      expect(calibrateButton()).toBeEnabled();
      const save = screen.getByRole('button', { name: 'Save as default' });
      expect(save).toBeEnabled();
      await user.click(save);
      await waitFor(() =>
        expect(ipc.captureLevelsSave).toHaveBeenCalledWith(
          'default',
          [212, 140, 5],
          CALIBRATED.exposure,
        ),
      );
    });

    it('shows a dash for a colour whose reading is unknown', async () => {
      const { user } = await ready();
      const id = await startCalibration(user);
      calibrationDone(id, { ...CALIBRATED, reached: [0.93, null, 0.5], warnings: [] });
      expect(await screen.findByText('Green 140: – of full scale in 2 shots')).toBeInTheDocument();
    });

    it('shows the reason when the calibration fails', async () => {
      const { user } = await ready();
      const id = await startCalibration(user);
      calibrationError(id, 'Io', 'Red is clipped even at level 1: the exposure is too long.');
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'Red is clipped even at level 1: the exposure is too long.',
      );
      expect(calibrateButton()).toBeEnabled();
    });

    it('shows the reason when it cannot start', async () => {
      const { user } = await ready();
      state.calibrationFail = 'The levels are out of range.';
      await user.click(calibrateButton());
      expect(await screen.findByRole('alert')).toHaveTextContent('The levels are out of range.');
    });

    it('says it was cancelled and leaves the levels as they were', async () => {
      const { user } = await ready();
      const id = await startCalibration(user);
      calibrationError(id, 'Cancelled', 'Cancelled');
      expect(await screen.findByText('Calibration cancelled.')).toBeInTheDocument();
      expect(screen.getByRole('slider', { name: 'Red level' })).toHaveValue(44);
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
      expect(calibrateButton()).toBeEnabled();
    });
  });

  describe('live view', () => {
    let urls: string[];
    let revoked: string[];
    beforeEach(() => {
      urls = [];
      revoked = [];
      URL.createObjectURL = vi.fn(() => {
        const url = `blob:frame-${urls.length}`;
        urls.push(url);
        return url;
      });
      URL.revokeObjectURL = vi.fn((url: string) => {
        revoked.push(url);
      });
    });

    const startLive = () => screen.getByRole('button', { name: 'Start live view' });
    const stopLive = () => screen.getByRole('button', { name: 'Stop live view' });

    it('shows frames and keeps asking for the next one', async () => {
      const { user } = await ready();
      await user.click(startLive());
      const image = await screen.findByRole('img', { name: 'Live view of the camera' });
      expect(image.getAttribute('src')).toMatch(/^blob:/);
      await waitFor(() => expect(ipc.cameraPreviewFrame.mock.calls.length).toBeGreaterThan(2));
      expect(await screen.findByText(/^\d+ frames\/s$/)).toBeInTheDocument();
      expect(screen.getByText(/Live view does not turn the light on/)).toBeInTheDocument();
    });

    it('asks for one frame at a time', async () => {
      const { user } = await ready();
      let open = 0;
      let most = 0;
      ipc.cameraPreviewFrame.mockImplementation(async () => {
        open += 1;
        most = Math.max(most, open);
        await new Promise((resolve) => setTimeout(resolve, 1));
        open -= 1;
        return new Uint8Array([0xff, 0xd8]).buffer;
      });
      await user.click(startLive());
      await waitFor(() => expect(ipc.cameraPreviewFrame.mock.calls.length).toBeGreaterThan(3));
      expect(most).toBe(1);
    });

    it('stops on the Stop button, leaves live view and releases the pictures', async () => {
      const { user } = await ready();
      await user.click(startLive());
      await screen.findByRole('img', { name: 'Live view of the camera' });
      await user.click(stopLive());
      expect(ipc.cameraPreviewStop).toHaveBeenCalledTimes(1);
      const asked = ipc.cameraPreviewFrame.mock.calls.length;
      await new Promise((resolve) => setTimeout(resolve, 20));
      expect(ipc.cameraPreviewFrame.mock.calls.length).toBeLessThanOrEqual(asked);
      expect(
        screen.queryByRole('img', { name: 'Live view of the camera' }),
      ).not.toBeInTheDocument();
      expect(startLive()).toBeEnabled();
      expect([...revoked].sort()).toEqual([...urls].sort());
    });

    it('stops when a test shot starts', async () => {
      const { user } = await ready();
      await user.click(startLive());
      await screen.findByRole('img', { name: 'Live view of the camera' });
      await user.click(screen.getByRole('button', { name: 'Test shot, red' }));
      await waitFor(() => expect(ipc.cameraPreviewStop).toHaveBeenCalledTimes(1));
      expect(startLive()).toBeInTheDocument();
    });

    it('stops when a capture starts', async () => {
      const { user } = await ready();
      await user.click(startLive());
      await screen.findByRole('img', { name: 'Live view of the camera' });
      await startCapture(user);
      expect(ipc.cameraPreviewStop).toHaveBeenCalledTimes(1);
      expect(startLive()).toBeDisabled();
    });

    it('stops when a calibration starts', async () => {
      const { user } = await ready();
      await user.click(startLive());
      await screen.findByRole('img', { name: 'Live view of the camera' });
      await startCalibration(user);
      expect(ipc.cameraPreviewStop).toHaveBeenCalledTimes(1);
      expect(startLive()).toBeDisabled();
    });

    it('cannot start while a capture runs', async () => {
      const { user } = await ready();
      await startCapture(user);
      expect(startLive()).toBeDisabled();
    });

    it('stops before the light goes off when the view closes', async () => {
      const { user, unmount } = await ready();
      await user.click(startLive());
      await screen.findByRole('img', { name: 'Live view of the camera' });
      unmount();
      expect(ipc.cameraPreviewStop).toHaveBeenCalledTimes(1);
      const stopOrder = vi.mocked(ipc.cameraPreviewStop).mock.invocationCallOrder[0] ?? Infinity;
      const offOrder = vi.mocked(ipc.lightOff).mock.invocationCallOrder[0] ?? -Infinity;
      expect(stopOrder).toBeLessThan(offOrder);
      expect([...revoked].sort()).toEqual([...urls].sort());
    });

    it('does not touch live view on close when it never ran', async () => {
      const { unmount } = await ready();
      unmount();
      expect(ipc.cameraPreviewStop).not.toHaveBeenCalled();
      expect(ipc.cameraPreviewFrame).not.toHaveBeenCalled();
    });

    it('stops and says why when a frame fails', async () => {
      const { user } = await ready();
      state.liveViewFail = 'A capture is in progress. Wait until it is done.';
      await user.click(startLive());
      expect(await screen.findByRole('alert')).toHaveTextContent('A capture is in progress.');
      expect(startLive()).toBeEnabled();
      const asked = ipc.cameraPreviewFrame.mock.calls.length;
      await new Promise((resolve) => setTimeout(resolve, 20));
      expect(ipc.cameraPreviewFrame.mock.calls.length).toBe(asked);
      expect(ipc.cameraPreviewStop).toHaveBeenCalledTimes(1);
    });

    describe('light control', () => {
      const choose = (name: string) =>
        within(screen.getByRole('group', { name: 'Light' })).getByRole('radio', { name });

      it('sends the levels of each choice', async () => {
        const { user } = await ready();
        expect(choose('Off')).toBeChecked();
        await user.click(choose('Red'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenLastCalledWith([44, 0, 0]));
        await user.click(choose('Green'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenLastCalledWith([0, 22, 0]));
        await user.click(choose('Blue'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenLastCalledWith([0, 0, 2]));
        await user.click(choose('All'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenLastCalledWith([44, 22, 2]));
        expect(choose('All')).toBeChecked();
      });

      it('works without live view running', async () => {
        const { user } = await ready();
        await user.click(choose('Green'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenCalled());
        expect(ipc.cameraPreviewFrame).not.toHaveBeenCalled();
      });

      it('sends the levels again when a slider moves', async () => {
        const { user } = await ready();
        await user.click(choose('Red'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenCalledTimes(1));
        screen.getByRole('slider', { name: 'Red level' }).focus();
        await user.keyboard('{ArrowRight}');
        await waitFor(() => expect(ipc.lightSet).toHaveBeenLastCalledWith([45, 0, 0]));
      });

      it('drops a send that a newer one supersedes while it is on its way', async () => {
        const { user } = await ready();
        let release: () => void = () => undefined;
        ipc.lightSet.mockImplementationOnce(
          () => new Promise<undefined>((resolve) => (release = () => resolve(undefined))),
        );
        await user.click(choose('Red'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenCalledTimes(1));
        await user.click(choose('Green'));
        await user.click(choose('Blue'));
        release();
        await waitFor(() => expect(ipc.lightSet).toHaveBeenCalledTimes(2));
        expect(ipc.lightSet).toHaveBeenLastCalledWith([0, 0, 2]);
      });

      it('switches the light off on Off', async () => {
        const { user } = await ready();
        await user.click(choose('Blue'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenCalled());
        await user.click(choose('Off'));
        await waitFor(() => expect(ipc.lightOff).toHaveBeenCalledTimes(1));
        expect(choose('Off')).toBeChecked();
      });

      it('switches the light off when live view is stopped', async () => {
        const { user } = await ready();
        await user.click(startLive());
        await user.click(choose('Green'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenCalled());
        await user.click(stopLive());
        await waitFor(() => expect(ipc.lightOff).toHaveBeenCalledTimes(1));
        expect(choose('Off')).toBeChecked();
      });

      it('switches the light off when a frame fails', async () => {
        const { user } = await ready();
        await user.click(choose('Green'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenCalled());
        state.liveViewFail = 'This camera\u2019s live view is not available from the app.';
        await user.click(startLive());
        await screen.findByText(/live view is not available/);
        await waitFor(() => expect(ipc.lightOff).toHaveBeenCalledTimes(1));
        expect(choose('Off')).toBeChecked();
      });

      it('reads Off without a switch-off when a test shot starts, and is disabled meanwhile', async () => {
        const { user } = await ready();
        await user.click(choose('Green'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenCalled());
        ipc.captureTest.mockImplementationOnce(() => new Promise(() => undefined));
        await user.click(screen.getByRole('button', { name: 'Test shot, red' }));
        await waitFor(() => expect(choose('Off')).toBeChecked());
        expect(choose('Red')).toBeDisabled();
        expect(ipc.lightOff).not.toHaveBeenCalled();
      });

      it('reads Off without a switch-off when a capture starts', async () => {
        const { user } = await ready();
        await user.click(choose('All'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenCalled());
        await startCapture(user);
        expect(choose('Off')).toBeChecked();
        expect(choose('Red')).toBeDisabled();
        expect(ipc.lightOff).not.toHaveBeenCalled();
      });

      it('switches the light off after stopping live view when the view closes', async () => {
        const { user, unmount } = await ready();
        await user.click(startLive());
        await user.click(choose('Red'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenCalled());
        unmount();
        expect(ipc.lightOff).toHaveBeenCalledTimes(1);
        const stopOrder = vi.mocked(ipc.cameraPreviewStop).mock.invocationCallOrder[0] ?? Infinity;
        const offOrder = vi.mocked(ipc.lightOff).mock.invocationCallOrder[0] ?? -Infinity;
        expect(stopOrder).toBeLessThan(offOrder);
      });

      it('shows the message of a refusal and returns to Off', async () => {
        const { user } = await ready();
        ipc.lightSet.mockRejectedValueOnce(
          new IpcError({ code: 'Internal', message: 'The light is too hot.' }),
        );
        await user.click(choose('Red'));
        expect(await screen.findByRole('alert')).toHaveTextContent('The light is too hot.');
        expect(choose('Off')).toBeChecked();
      });

      it('reads Off when the light trips', async () => {
        const { user } = await ready();
        await user.click(choose('Red'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenCalled());
        emitLight(reading({ tripped: true }));
        await waitFor(() => expect(choose('Off')).toBeChecked());
      });

      it('has no accessibility violations with the light on', async () => {
        const { user, container } = await ready();
        await user.click(startLive());
        await user.click(choose('Green'));
        await waitFor(() => expect(ipc.lightSet).toHaveBeenCalled());
        await screen.findByRole('img', { name: 'Live view of the camera' });
        await expectNoA11yViolations(container);
      });
    });

    describe('focus aids', () => {
      // jsdom decodes no images and draws no canvas: a frame is a checkerboard of this contrast.
      let contrast = 100;
      const SIZE = { width: 64, height: 48 };
      const context = {
        drawImage: vi.fn(),
        getImageData: vi.fn(() => {
          const data = new Uint8ClampedArray(SIZE.width * SIZE.height * 4);
          for (let p = 0; p < SIZE.width * SIZE.height; p += 1) {
            const x = p % SIZE.width;
            const y = Math.floor(p / SIZE.width);
            const v = (x + y) % 2 === 0 ? contrast : 0;
            data.set([v, v, v, 255], p * 4);
          }
          return { data };
        }),
        createImageData: vi.fn(() => ({
          data: new Uint8ClampedArray(SIZE.width * SIZE.height * 4),
        })),
        putImageData: vi.fn(),
        clearRect: vi.fn(),
      };
      const realGetContext = HTMLCanvasElement.prototype.getContext;
      const realBitmap = globalThis.createImageBitmap;
      const number = (v: number) =>
        v.toLocaleString(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 1 });

      beforeEach(() => {
        contrast = 100;
        Object.values(context).forEach((fn) => fn.mockClear());
        HTMLCanvasElement.prototype.getContext = vi.fn(() => context) as never;
        globalThis.createImageBitmap = vi.fn(async () => ({
          ...SIZE,
          close: () => undefined,
        })) as never;
      });
      afterEach(() => {
        HTMLCanvasElement.prototype.getContext = realGetContext;
        globalThis.createImageBitmap = realBitmap;
      });

      const begin = async () => {
        const view = await ready();
        await view.user.click(startLive());
        await screen.findByRole('img', { name: 'Live view of the camera' });
        return view;
      };

      it('draws the edges on the overlay only when peaking is on', async () => {
        const { user } = await begin();
        await waitFor(() => expect(globalThis.createImageBitmap).toHaveBeenCalled());
        expect(context.putImageData).not.toHaveBeenCalled();
        expect(screen.queryByLabelText('Peaking sensitivity')).not.toBeInTheDocument();
        await user.click(screen.getByRole('checkbox', { name: 'Focus peaking' }));
        expect(screen.getByLabelText('Peaking sensitivity')).toBeInTheDocument();
        await waitFor(() => expect(context.putImageData).toHaveBeenCalled());
        await user.click(screen.getByRole('checkbox', { name: 'Focus peaking' }));
        await waitFor(() => expect(context.clearRect).toHaveBeenCalled());
      });

      it('shows the sharpness, the best value, and starts the best again on Reset best', async () => {
        const { user } = await begin();
        const sharp = 16 * 100 * 100;
        expect(await screen.findByText(`Sharpness ${number(sharp)}`)).toBeInTheDocument();
        expect(screen.getByText(`Best ${number(sharp)}`)).toBeInTheDocument();
        contrast = 50;
        const soft = 16 * 50 * 50;
        expect(await screen.findByText(`Sharpness ${number(soft)}`)).toBeInTheDocument();
        expect(screen.getByText(`Best ${number(sharp)}`)).toBeInTheDocument();
        await user.click(screen.getByRole('button', { name: 'Reset best' }));
        expect(await screen.findByText(`Best ${number(soft)}`)).toBeInTheDocument();
      });

      it('has no accessibility violations with peaking and the zoom on', async () => {
        const { user, container } = await begin();
        await user.click(screen.getByRole('checkbox', { name: 'Focus peaking' }));
        await screen.findByRole('radio', { name: '100 %' });
        await user.click(screen.getByRole('radio', { name: '100 %' }));
        await waitFor(() => expect(screen.getByRole('radio', { name: '100 %' })).toBeChecked());
        await screen.findByText(/^Best /);
        await expectNoA11yViolations(container);
      });
    });

    describe('zoom', () => {
      const begin = async () => {
        const view = await ready();
        await view.user.click(startLive());
        await screen.findByRole('img', { name: 'Live view of the camera' });
        await screen.findByRole('radio', { name: 'Whole frame' });
        return view;
      };
      const pick = () => screen.getByRole('button', { name: /^Choose the spot to zoom to/ });

      it('offers the steps the camera gives, whole frame first', async () => {
        await begin();
        expect(
          screen.getAllByRole('radio', { name: /frame|%/ }).map((r) => r.getAttribute('id')),
        ).toHaveLength(5);
        expect(screen.getByRole('radio', { name: 'Whole frame' })).toBeChecked();
        expect(screen.getByRole('radio', { name: '200 %' })).toBeInTheDocument();
      });

      it('shows no zoom controls when the camera has no zoom', async () => {
        state.zoomSteps = [];
        const { user } = await ready();
        await user.click(startLive());
        await screen.findByRole('img', { name: 'Live view of the camera' });
        await waitFor(() => expect(ipc.cameraPreviewZoomSteps).toHaveBeenCalled());
        expect(screen.queryByRole('radio', { name: 'Whole frame' })).not.toBeInTheDocument();
        expect(screen.queryByRole('button', { name: 'Left' })).not.toBeInTheDocument();
        expect(screen.queryByRole('button', { name: /^Choose the spot/ })).not.toBeInTheDocument();
      });

      it('sets the zoom to the value of the chosen step', async () => {
        const { user } = await begin();
        await user.click(screen.getByRole('radio', { name: '100 %' }));
        await waitFor(() => expect(ipc.cameraPreviewSetZoom).toHaveBeenCalledWith('6'));
        expect(screen.getByRole('radio', { name: '100 %' })).toBeChecked();
      });

      it('shows the message when the zoom is refused, and keeps the step', async () => {
        const { user } = await begin();
        ipc.cameraPreviewSetZoom.mockRejectedValueOnce(
          new IpcError({ code: 'Internal', message: 'The camera refused the zoom.' }),
        );
        await user.click(screen.getByRole('radio', { name: '50 %' }));
        expect(await screen.findByRole('alert')).toHaveTextContent('The camera refused the zoom.');
        expect(screen.getByRole('radio', { name: 'Whole frame' })).toBeChecked();
      });

      it('aims at the spot clicked on the whole frame and marks it', async () => {
        await begin();
        const box = { left: 100, top: 50, width: 200, height: 100 } as DOMRect;
        vi.spyOn(pick(), 'getBoundingClientRect').mockReturnValue(box);
        fireEvent.click(pick(), { clientX: 150, clientY: 75, detail: 1 });
        await waitFor(() => expect(ipc.cameraPreviewSetArea).toHaveBeenCalledWith(0.25, 0.25));
        await waitFor(() =>
          expect(document.querySelector('[class*="captureLiveMarker"]')).not.toBeNull(),
        );
      });

      it('aims at the middle when the button is pressed with the keyboard', async () => {
        await begin();
        fireEvent.click(pick(), { detail: 0 });
        await waitFor(() => expect(ipc.cameraPreviewSetArea).toHaveBeenCalledWith(0.5, 0.5));
      });

      it('nudges the zoomed spot by a quarter of the view', async () => {
        const { user } = await begin();
        await user.click(screen.getByRole('radio', { name: '100 %' }));
        await waitFor(() => expect(ipc.cameraPreviewSetZoom).toHaveBeenCalled());
        expect(screen.queryByRole('button', { name: /^Choose the spot/ })).not.toBeInTheDocument();
        await user.click(screen.getByRole('button', { name: 'Right' }));
        await waitFor(() => expect(ipc.cameraPreviewSetArea).toHaveBeenCalledTimes(1));
        const step = 0.25 * (1 / (100 * 0.094));
        const [x1, y1] = ipc.cameraPreviewSetArea.mock.calls[0] ?? [];
        expect(x1).toBeCloseTo(0.5 + step, 5);
        expect(y1).toBe(0.5);
        await user.click(screen.getByRole('button', { name: 'Up' }));
        await waitFor(() => expect(ipc.cameraPreviewSetArea).toHaveBeenCalledTimes(2));
        const [x2, y2] = ipc.cameraPreviewSetArea.mock.calls[1] ?? [];
        expect(x2).toBeCloseTo(0.5 + step, 5);
        expect(y2).toBeCloseTo(0.5 - step, 5);
      });

      it('is back at the whole frame after Stop and a new start', async () => {
        const { user } = await begin();
        await user.click(screen.getByRole('radio', { name: '50 %' }));
        await waitFor(() => expect(screen.getByRole('radio', { name: '50 %' })).toBeChecked());
        await user.click(stopLive());
        expect(screen.queryByRole('radio', { name: '50 %' })).not.toBeInTheDocument();
        await user.click(startLive());
        await screen.findByRole('radio', { name: 'Whole frame' });
        expect(screen.getByRole('radio', { name: 'Whole frame' })).toBeChecked();
      });

      it('goes away with live view when a capture starts', async () => {
        const { user } = await begin();
        await user.click(screen.getByRole('radio', { name: '25 %' }));
        await waitFor(() => expect(ipc.cameraPreviewSetZoom).toHaveBeenCalled());
        await startCapture(user);
        expect(screen.queryByRole('radio', { name: '25 %' })).not.toBeInTheDocument();
        expect(screen.queryByRole('button', { name: 'Left' })).not.toBeInTheDocument();
        expect(ipc.cameraPreviewStop).toHaveBeenCalledTimes(1);
      });

      it('goes back to the whole frame when a frame fails', async () => {
        const { user } = await begin();
        await user.click(screen.getByRole('radio', { name: '25 %' }));
        await waitFor(() => expect(ipc.cameraPreviewSetZoom).toHaveBeenCalled());
        state.liveViewFail =
          'Live view cannot start: Sequence error. Switch the camera off and on, then try again.';
        expect(await screen.findByRole('alert')).toHaveTextContent('Sequence error');
        expect(screen.queryByRole('radio', { name: '25 %' })).not.toBeInTheDocument();
      });

      it('goes back to the whole frame when a camera setting changes, and keeps running', async () => {
        const { user } = await begin();
        await user.click(screen.getByRole('radio', { name: '100 %' }));
        await waitFor(() => expect(screen.getByRole('radio', { name: '100 %' })).toBeChecked());
        await user.selectOptions(await screen.findByRole('combobox', { name: 'ISO' }), '400');
        await waitFor(() => expect(ipc.cameraSettingsSet).toHaveBeenCalledWith('iso', '400'));
        await waitFor(() =>
          expect(screen.getByRole('radio', { name: 'Whole frame' })).toBeChecked(),
        );
        expect(stopLive()).toBeInTheDocument();
      });

      it('leaves live view when the view closes while zoomed', async () => {
        const { user, unmount } = await begin();
        await user.click(screen.getByRole('radio', { name: '25 %' }));
        await waitFor(() => expect(ipc.cameraPreviewSetZoom).toHaveBeenCalled());
        unmount();
        expect(ipc.cameraPreviewStop).toHaveBeenCalledTimes(1);
      });
    });

    it('is not offered with the folder camera backend', async () => {
      state.profile = { ...state.profile, camera_backend: 'folder' };
      await ready();
      expect(screen.getByText('Live view is available on macOS only for now.')).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Start live view' })).not.toBeInTheDocument();
    });

    it('has no accessibility violations while running', async () => {
      const { user, container } = await ready();
      await user.click(startLive());
      await screen.findByRole('img', { name: 'Live view of the camera' });
      await expectNoA11yViolations(container);
    });
  });

  describe('focus check', () => {
    const checkButton = () => screen.getByRole('button', { name: /^(Focus check|Checking)/ });
    const colours = () => within(screen.getByRole('group', { name: 'Focus check' }));
    const run = async (user: ReturnType<typeof userEvent.setup>) => {
      await user.click(checkButton());
      return screen.findByRole('img', { name: /full size/ });
    };
    const pending = () =>
      ipc.captureFocusCheck.mockImplementationOnce(() => new Promise(() => undefined));
    const number = (v: number) =>
      v.toLocaleString(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 1 });

    it('takes the check in the chosen colour at the working levels, green by default', async () => {
      const { user } = await ready();
      expect(colours().getByRole('radio', { name: 'Green' })).toBeChecked();
      await run(user);
      expect(ipc.captureFocusCheck).toHaveBeenLastCalledWith('green', [44, 22, 2]);
      await user.click(colours().getByRole('radio', { name: 'Blue' }));
      await run(user);
      expect(ipc.captureFocusCheck).toHaveBeenLastCalledWith('blue', [44, 22, 2]);
    });

    it('is busy while the picture is taken and stops live view', async () => {
      const { user } = await ready();
      await user.click(screen.getByRole('button', { name: 'Start live view' }));
      await waitFor(() => expect(ipc.cameraPreviewFrame).toHaveBeenCalled());
      pending();
      await user.click(checkButton());
      expect(await screen.findByText('Taking a picture…')).toBeInTheDocument();
      expect(checkButton()).toBeDisabled();
      expect(colours().getByRole('radio', { name: 'Red' })).toBeDisabled();
      await waitFor(() => expect(ipc.cameraPreviewStop).toHaveBeenCalledTimes(1));
      expect(screen.getByRole('button', { name: 'Start live view' })).toBeDisabled();
    });

    it('shows the crop at full size, the number and the best so far, which can be reset', async () => {
      const { user } = await ready();
      const image = await run(user);
      expect(image).toHaveAttribute('src', 'data:image/jpeg;base64,AAAA');
      expect(image).toHaveAttribute('width', '512');
      expect(screen.getByText(new RegExp(`Sharpness ${number(12.34)}\\.`))).toBeInTheDocument();
      expect(screen.getByText(new RegExp(`Best so far ${number(12.34)}`))).toBeInTheDocument();
      state.focusCheck = { ...state.focusCheck, sharpness: 5 };
      await run(user);
      expect(screen.getByText(new RegExp(`Sharpness ${number(5)}\\.`))).toBeInTheDocument();
      expect(screen.getByText(new RegExp(`Best so far ${number(12.34)}`))).toBeInTheDocument();
      await user.click(screen.getAllByRole('button', { name: 'Reset best' }).at(-1) as HTMLElement);
      expect(screen.queryByText(/Best so far/)).not.toBeInTheDocument();
    });

    it('shows the map as a table with the share of each part', async () => {
      const { user } = await ready();
      await run(user);
      const table = screen.getByRole('table');
      expect(within(table).getByRole('columnheader', { name: 'Centre' })).toBeInTheDocument();
      expect(within(table).getByRole('rowheader', { name: 'Middle' })).toBeInTheDocument();
      const rows = within(table).getAllByRole('row');
      expect(
        within(rows[1] as HTMLElement)
          .getAllByRole('cell')
          .map((c) => c.textContent),
      ).toEqual(['80 %', '90 %', '80 %']);
      expect(within(rows[2] as HTMLElement).getAllByRole('cell')[1]).toHaveTextContent('100 %');
    });

    it.each([
      [{ kind: 'flat' }, 'The film looks flat and square to the camera.'],
      [
        { kind: 'bowed' },
        'The middle is sharper than the edges: the film is bowed or the holder sags.',
      ],
      [
        { kind: 'tilted', side: 'left' },
        'The left side is softer: the film or the holder is tilted.',
      ],
      [
        { kind: 'tilted', side: 'bottom_right' },
        'The bottom right corner is softer: the film or the holder is tilted.',
      ],
      [{ kind: 'unclear' }, 'No clear pattern.'],
    ])('names the pattern %j', async (pattern, hint) => {
      const { user } = await ready();
      state.focusCheck = { ...state.focusCheck, pattern } as never;
      await run(user);
      expect(screen.getByText(hint)).toBeInTheDocument();
    });

    it('says when the map is fair and the aperture that was used', async () => {
      const { user } = await ready();
      await run(user);
      expect(screen.getByText(/Fair only on even texture/)).toHaveTextContent('f/4 to f/5.6');
      expect(screen.getByText(/Aperture used: f\/5\.6\./)).toBeInTheDocument();
    });

    it('saves the map without the reference as the flat reference, and clears it', async () => {
      const { user } = await ready();
      await run(user);
      expect(screen.queryByRole('button', { name: 'Clear reference' })).not.toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Save as flat reference' }));
      await waitFor(() =>
        expect(ipc.settingsSet).toHaveBeenCalledWith({ focus_reference: state.focusCheck.raw_map }),
      );
      expect(await screen.findByText('A flat reference is saved.')).toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Clear reference' }));
      await waitFor(() =>
        expect(ipc.settingsSet).toHaveBeenLastCalledWith({ clear_focus_reference: true }),
      );
      await waitFor(() =>
        expect(screen.queryByRole('button', { name: 'Clear reference' })).not.toBeInTheDocument(),
      );
    });

    it('offers to clear a reference that was saved before', async () => {
      state.focusReference = state.focusCheck.raw_map;
      await ready();
      expect(await screen.findByRole('button', { name: 'Clear reference' })).toBeInTheDocument();
    });

    it('says when the map is compared with the flat reference', async () => {
      const { user } = await ready();
      await run(user);
      expect(screen.queryByText('Compared with the flat reference.')).not.toBeInTheDocument();
      state.focusCheck = { ...state.focusCheck, against_reference: true };
      await run(user);
      expect(screen.getByText('Compared with the flat reference.')).toBeInTheDocument();
    });

    it('shows the message when the check fails', async () => {
      const { user } = await ready();
      state.focusFail = 'The camera did not answer.';
      await user.click(checkButton());
      expect(await screen.findByRole('alert')).toHaveTextContent('The camera did not answer.');
      expect(checkButton()).toBeEnabled();
    });

    it('shows the message when the reference cannot be saved', async () => {
      const { user } = await ready();
      await run(user);
      state.settingsRefused = 'Settings are read only.';
      await user.click(screen.getByRole('button', { name: 'Save as flat reference' }));
      expect(await screen.findByRole('alert')).toHaveTextContent('Settings are read only.');
    });

    it('cannot start while a capture runs', async () => {
      const { user } = await ready();
      await startCapture(user);
      expect(checkButton()).toBeDisabled();
    });

    it('has no accessibility violations with a result shown', async () => {
      const { user, container } = await ready();
      await run(user);
      await expectNoA11yViolations(container);
    });
  });

  describe('lifetime', () => {
    it('switches the light off when the view closes', async () => {
      const { unmount } = await ready();
      expect(ipc.lightOff).not.toHaveBeenCalled();
      unmount();
      expect(ipc.lightOff).toHaveBeenCalledTimes(1);
      expect(listeners.light).toHaveLength(0);
      expect(listeners.step).toHaveLength(0);
    });

    it('cancels a running capture before switching off when the view closes', async () => {
      const { user, unmount } = await ready();
      const id = await startCapture(user);
      expect(ipc.jobCancel).not.toHaveBeenCalled();
      unmount();
      expect(ipc.jobCancel).toHaveBeenCalledWith(id);
      expect(ipc.lightOff).toHaveBeenCalledTimes(1);
      const cancelOrder = vi.mocked(ipc.jobCancel).mock.invocationCallOrder[0] ?? Infinity;
      const offOrder = vi.mocked(ipc.lightOff).mock.invocationCallOrder[0] ?? -Infinity;
      expect(cancelOrder).toBeLessThan(offOrder);
    });

    it('cancels a running calibration before switching off when the view closes', async () => {
      const { user, unmount } = await ready();
      const id = await startCalibration(user);
      expect(ipc.jobCancel).not.toHaveBeenCalled();
      unmount();
      expect(ipc.jobCancel).toHaveBeenCalledWith(id);
      expect(ipc.lightOff).toHaveBeenCalledTimes(1);
      const cancelOrder = vi.mocked(ipc.jobCancel).mock.invocationCallOrder[0] ?? Infinity;
      const offOrder = vi.mocked(ipc.lightOff).mock.invocationCallOrder[0] ?? -Infinity;
      expect(cancelOrder).toBeLessThan(offOrder);
    });

    it('does not even look for the light when the flag is off', async () => {
      state.flags = { ...state.flags, guided_capture: false };
      openLibrary();
      const { container, unmount } = render(<Capture />);
      await waitFor(() => expect(ipc.settingsGet).toHaveBeenCalled());
      expect(container).toBeEmptyDOMElement();
      unmount();
      expect(ipc.cameraList).not.toHaveBeenCalled();
      expect(ipc.lightOff).not.toHaveBeenCalled();
    });
  });

  it('has no accessibility violations', async () => {
    const { container } = await ready();
    await expectNoA11yViolations(container);
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Test shot, red' }));
    await screen.findByRole('img', { name: /Histogram/ });
    await expectNoA11yViolations(container);
  });

  it('has no accessibility violations while calibrating and after', async () => {
    const { container, user } = await ready();
    const id = await startCalibration(user);
    calibrationProgress(id, 'Red: shot 1, level 44', 1 / 18);
    await screen.findByText('Red: shot 1, level 44');
    await expectNoA11yViolations(container);
    calibrationDone(id);
    await screen.findByText(/Blue ended at level 5/);
    await expectNoA11yViolations(container);
  });

  it('has no accessibility violations with the camera settings, an absent one and an error', async () => {
    state.cameraSettings = state.cameraSettings.map((s) =>
      s.kind === 'aperture' ? { kind: 'aperture', value: null, choices: [], writable: false } : s,
    );
    const { container } = await ready();
    await screen.findByRole('combobox', { name: 'ISO' });
    expect(screen.getByText('Set on the lens')).toBeInTheDocument();
    await expectNoA11yViolations(container);
    state.cameraSettingsSetFail = 'Refused.';
    const user = userEvent.setup();
    await user.selectOptions(screen.getByRole('combobox', { name: 'ISO' }), '200');
    await screen.findByRole('alert');
    await expectNoA11yViolations(container);
  });
});
