import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../test-support';
import i18n from '../../shared/i18n';
import type { ImportWarning } from '../../shared/ipc';
import { ipc, libraryDetails, listeners, makeRoll, resetFake, state } from '../library/fake-ipc';
import { resetLibraryStore, useLibraryStore } from '../library/library-store';
import { useLibraryEvents } from '../library/use-library-events';
import { warningTexts } from './capture/NextFrameButton';
import { CaptureSection } from './CaptureSection';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

function Capture() {
  useLibraryEvents(true);
  return <CaptureSection />;
}

function openLibrary(rollOver = {}, libraryOver = {}) {
  state.details = libraryDetails(libraryOver);
  const roll = makeRoll({ name: 'Harbour', ...rollOver });
  state.rolls = [roll];
  useLibraryStore.setState({
    phase: 'ready',
    library: state.details,
    rolls: [roll],
    rollId: roll.id,
  });
  return roll;
}

/** Renders the section with a light that answers and a camera that is there. */
async function ready(rollOver = {}, libraryOver = {}) {
  const user = userEvent.setup();
  const roll = openLibrary(rollOver, libraryOver);
  const view = render(<Capture />);
  await screen.findByText('Camera: Nikon D600');
  await user.click(screen.getByRole('button', { name: 'Connect light' }));
  await screen.findByText('Light connected');
  return { roll, user, ...view };
}

const flatButton = () => screen.getByRole('button', { name: 'Capture flat field' });

/** Opens the dialog, confirms, and returns the job id the fake gave. */
async function startFlatField(user: ReturnType<typeof userEvent.setup>) {
  await user.click(flatButton());
  const dialog = await screen.findByRole('alertdialog');
  await user.click(within(dialog).getByRole('button', { name: 'Capture' }));
  await waitFor(() => expect(ipc.captureFlatField).toHaveBeenCalled());
  await waitFor(() => expect(useLibraryStore.getState().jobs).toHaveLength(1));
  return state.flatFieldJob;
}

const finish = (
  id: string,
  f_number: { num: number; den: number } | null,
  levels: [number, number, number] = [40, 20, 3],
) =>
  act(() => {
    listeners.done.forEach((cb) =>
      cb({
        id,
        result: {
          kind: 'FlatField',
          info: {
            frame_width: 6016,
            frame_height: 4016,
            sources: ['r.NEF', 'g.NEF', 'b.NEF'],
            exposure: { exposure_time: { num: 1, den: 60 }, iso: 100, f_number },
          },
          levels,
        } as never,
      }),
    );
  });

const progress = (id: string, stage: string, fraction: number) =>
  act(() => listeners.progress.forEach((cb) => cb({ id, stage, fraction })));

describe('Capture flat field', () => {
  beforeEach(() => {
    resetFake();
    resetLibraryStore();
  });

  describe('the button', () => {
    it('is off before the light is connected', async () => {
      openLibrary();
      render(<Capture />);
      await screen.findByText('Camera: Nikon D600');
      expect(flatButton()).toBeDisabled();
    });

    it('is off without a camera', async () => {
      state.cameras = [];
      openLibrary();
      const user = userEvent.setup();
      render(<Capture />);
      await screen.findByText('No camera found');
      await user.click(screen.getByRole('button', { name: 'Connect light' }));
      await screen.findByText('Light connected');
      expect(flatButton()).toBeDisabled();
    });

    it('is off without a roll', async () => {
      const user = userEvent.setup();
      state.details = libraryDetails();
      useLibraryStore.setState({ phase: 'ready', library: state.details, rolls: [], rollId: null });
      render(<Capture />);
      await screen.findByText('Camera: Nikon D600');
      await user.click(screen.getByRole('button', { name: 'Connect light' }));
      await screen.findByText('Light connected');
      expect(flatButton()).toBeDisabled();
    });

    it('is off in a read-only library', async () => {
      await ready({}, { read_only: true });
      expect(flatButton()).toBeDisabled();
      expect(
        screen.getByText('The library is read-only, so no flat field can be captured.'),
      ).toBeInTheDocument();
    });

    it('is on when everything is there, and off while a capture runs', async () => {
      const { roll, user } = await ready();
      expect(flatButton()).toBeEnabled();
      useLibraryStore.getState().addJob({
        id: 'job-x',
        kind: 'capture',
        rollName: roll.name,
        fraction: 0,
        state: 'running',
      });
      await waitFor(() => expect(flatButton()).toBeDisabled());
      expect(user).toBeDefined();
    });

    it('is off while the flat field itself runs', async () => {
      const { user } = await ready();
      await startFlatField(user);
      expect(flatButton()).toBeDisabled();
    });
  });

  describe('the dialog', () => {
    it('says to take the film out and leave the rest as it is', async () => {
      const { user } = await ready();
      await user.click(flatButton());
      const dialog = await screen.findByRole('alertdialog', { name: 'Capture the flat field' });
      expect(dialog).toHaveTextContent('Take the film out of the carrier.');
      expect(dialog).toHaveTextContent(
        'Leave the carrier, the camera, the focus and the light as they are.',
      );
      expect(within(dialog).queryByText(/replaces it/)).not.toBeInTheDocument();
    });

    it('says the light is calibrated for the bare light first and the sliders stay', async () => {
      const { user } = await ready();
      await user.click(flatButton());
      const dialog = await screen.findByRole('alertdialog');
      expect(dialog).toHaveTextContent('The light is calibrated for the bare light first');
      expect(dialog).toHaveTextContent('The sliders and the saved levels are not changed.');
    });

    it('says the new flat field replaces the roll’s own', async () => {
      const { user } = await ready({ has_flat_field: true });
      await user.click(flatButton());
      expect(
        await screen.findByText('This roll has a flat field. The new one replaces it.'),
      ).toBeInTheDocument();
    });

    it('starts nothing on Cancel', async () => {
      const { user } = await ready();
      await user.click(flatButton());
      const dialog = await screen.findByRole('alertdialog');
      await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
      await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument());
      expect(ipc.captureFlatField).not.toHaveBeenCalled();
    });

    it('is axe clean', async () => {
      const { user } = await ready({ has_flat_field: true });
      await user.click(flatButton());
      const dialog = await screen.findByRole('alertdialog');
      await expectNoA11yViolations(dialog);
    });
  });

  describe('the capture', () => {
    it('shows the calibration, then follows the steps of the three pictures', async () => {
      const { roll, user } = await ready();
      const id = await startFlatField(user);
      expect(ipc.captureFlatField).toHaveBeenCalledWith(roll.id, [44, 22, 2]);
      expect(useLibraryStore.getState().jobs[0]).toMatchObject({ id, kind: 'flat_field' });

      // The calibration shoots one colour at a time too: its steps are not the flat field's.
      progress(id, 'Calibrating red: shot 2, level 120', 0.05);
      expect(await screen.findByText('Calibrating red: shot 2, level 120')).toBeInTheDocument();
      act(() => listeners.step.forEach((cb) => cb({ channel: 'blue', state: 'received' })));
      expect(screen.queryByRole('list', { name: 'Steps' })).not.toBeInTheDocument();
      expect(screen.queryByText('red: light on')).not.toBeInTheDocument();

      progress(id, 'Capturing the flat field', 0.6);
      expect(await screen.findByText('Capturing the flat field')).toBeInTheDocument();
      expect(screen.queryByText('Calibrating red: shot 2, level 120')).not.toBeInTheDocument();
      expect(await screen.findByText('blue: waiting')).toBeInTheDocument();
      act(() => listeners.step.forEach((cb) => cb({ channel: 'red', state: 'lit' })));
      expect(await screen.findByText('red: light on')).toBeInTheDocument();
      expect(screen.getByText('blue: waiting')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Cancel flat field' })).toBeEnabled();
    });

    it('shows the aperture and the levels in the success notice and reloads the rolls', async () => {
      const { user } = await ready();
      const id = await startFlatField(user);
      ipc.rollList.mockClear();
      finish(id, { num: 8, den: 1 });
      expect(
        await screen.findByText('Flat field captured at f/8, levels 40, 20, 3.'),
      ).toBeInTheDocument();
      await waitFor(() => expect(ipc.rollList).toHaveBeenCalled());
      await waitFor(() => expect(useLibraryStore.getState().rolls[0]?.has_flat_field).toBe(true));
      expect(useLibraryStore.getState().rolls[0]?.flat_field_f_number).toEqual({
        num: 56,
        den: 10,
      });
      expect(flatButton()).toBeEnabled();
    });

    it('leaves the aperture out of the notice when it is unknown', async () => {
      const { user } = await ready();
      const id = await startFlatField(user);
      finish(id, null, [250, 168, 90]);
      expect(
        await screen.findByText('Flat field captured, levels 250, 168, 90.'),
      ).toBeInTheDocument();
    });

    it('shows the message of a failed capture', async () => {
      const { user } = await ready();
      const id = await startFlatField(user);
      act(() =>
        listeners.error.forEach((cb) =>
          cb({
            id,
            error: { code: 'InvalidInput', message: 'The red picture is clipped.' } as never,
          }),
        ),
      );
      expect(await screen.findByRole('alert')).toHaveTextContent('The red picture is clipped.');
    });

    it('shows the message when the capture is refused at once', async () => {
      state.flatFieldFail = 'The camera is busy.';
      const { user } = await ready();
      await user.click(flatButton());
      const dialog = await screen.findByRole('alertdialog');
      await user.click(within(dialog).getByRole('button', { name: 'Capture' }));
      expect(await screen.findByRole('alert')).toHaveTextContent('The camera is busy.');
      expect(useLibraryStore.getState().jobs).toHaveLength(0);
    });

    it('cancels the running capture', async () => {
      const { user } = await ready();
      const id = await startFlatField(user);
      await user.click(screen.getByRole('button', { name: 'Cancel flat field' }));
      expect(ipc.jobCancel).toHaveBeenCalledWith(id);
      act(() =>
        listeners.error.forEach((cb) =>
          cb({ id, error: { code: 'Cancelled', message: 'Cancelled' } as never }),
        ),
      );
      expect(await screen.findByText('The flat field capture was cancelled.')).toBeInTheDocument();
    });
  });

  describe('the aperture warning', () => {
    it('names both apertures', () => {
      const warnings: ImportWarning[] = [
        {
          frame_id: 'f',
          kind: {
            kind: 'flat_field_aperture_differs',
            flat: { num: 56, den: 10 },
            frame: { num: 8, den: 1 },
          },
        },
      ];
      expect(warningTexts(warnings, i18n.t.bind(i18n))).toEqual([
        'The flat field was shot at f/5.6 and this frame at f/8: it may correct the light wrongly.',
      ]);
    });
  });
});
