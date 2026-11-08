import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../test-support';
import type { ImportWarning, LightStatus } from '../../shared/ipc';
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
      expect(select('Shutter speed')).toHaveValue('1/100');
      expect(select('Aperture')).toHaveValue('8');
      const options = (el: HTMLElement) =>
        Array.from(el.querySelectorAll('option')).map((o) => o.textContent);
      expect(options(iso)).toEqual(['100', '200', '400', '800']);
      expect(options(select('Aperture'))).toEqual(['5.6', '8', '11']);
      expect(ipc.cameraSettingsGet).toHaveBeenCalled();
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
        if (shutter) shutter.value = '1/250';
        return settings;
      });
      await user.selectOptions(select('Aperture'), '11');
      await waitFor(() => expect(select('Shutter speed')).toHaveValue('1/250'));
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
      expect(select('Shutter speed')).toHaveValue('1/100');
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
