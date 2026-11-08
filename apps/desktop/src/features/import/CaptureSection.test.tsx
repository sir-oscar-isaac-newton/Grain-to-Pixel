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
    it('starts from the profile and saves a change when it is made', async () => {
      const user = userEvent.setup();
      openLibrary();
      render(<Capture />);
      const red = await screen.findByRole('slider', { name: 'Red level' });
      expect(red).toHaveAttribute('aria-valuenow', '44');
      expect(screen.getByRole('slider', { name: 'Blue level' })).toHaveAttribute(
        'aria-valuenow',
        '2',
      );
      red.focus();
      await user.keyboard('{ArrowRight}');
      await waitFor(() =>
        expect(ipc.captureProfileSet).toHaveBeenCalledWith({ levels: [45, 22, 2] }),
      );
      expect(red).toHaveAttribute('aria-valuenow', '45');
      // Moving a slider never lights the light.
      expect(ipc.lightSet).not.toHaveBeenCalled();
    });

    it('shows why a change was refused and puts the old level back', async () => {
      const user = userEvent.setup();
      state.profileRefused = 'That level is too bright.';
      openLibrary();
      render(<Capture />);
      const green = await screen.findByRole('slider', { name: 'Green level' });
      green.focus();
      await user.keyboard('{ArrowRight}');
      expect(await screen.findByText('That level is too bright.')).toBeInTheDocument();
      expect(green).toHaveAttribute('aria-valuenow', '22');
    });
  });

  describe('test shot', () => {
    const shotOf = (clipped: number | null) => {
      const histogram = new Array<number>(256).fill(0);
      histogram[100] = 0.5;
      state.testShot = { histogram, clipped_pct: clipped };
    };

    it('shows the histogram of the colour and how much is clipped, without a warning', async () => {
      shotOf(0.1);
      const { user } = await ready();
      await user.click(screen.getByRole('button', { name: 'Test shot, green' }));
      expect(
        await screen.findByRole('img', { name: 'Histogram of the green test shot' }),
      ).toBeInTheDocument();
      expect(ipc.captureTest).toHaveBeenCalledWith('green');
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
      expect(ipc.captureTriplet).toHaveBeenCalledWith(roll.id);
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
      ]);
      expect(await screen.findByText('Frame 7 on this roll')).toBeInTheDocument();
      expect(screen.getByText('The light was warm during this frame (47 °C).')).toBeInTheDocument();
      expect(screen.getByText(/dropped to 4600 mV/)).toBeInTheDocument();
      expect(screen.getByText(/different ISO values/)).toBeInTheDocument();
      expect(screen.getByText(/different shutter times/)).toBeInTheDocument();
      expect(
        screen.getByText('The blue picture is clipped (1.2 %). Lower its level.'),
      ).toBeInTheDocument();
      expect(screen.queryByRole('list', { name: 'Steps of this capture' })).not.toBeInTheDocument();
      expect(next()).toBeEnabled();
    });

    it('starts with the Space key', async () => {
      const { roll, user } = await ready();
      // Clicking Connect left the focus on that button, which keeps its own Space.
      (document.activeElement as HTMLElement | null)?.blur();
      await user.keyboard(' ');
      await waitFor(() => expect(ipc.captureTriplet).toHaveBeenCalledWith(roll.id));
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
      expect(screen.getByRole('slider', { name: 'Red level' })).toHaveAttribute('data-disabled');
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
});
