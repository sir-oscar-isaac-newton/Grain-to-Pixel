import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import { expectNoA11yViolations } from '../../test-support';
import { ipc, libraryDetails, listeners, resetFake, state } from '../library/fake-ipc';
import { resetLibraryStore, useLibraryStore } from '../library/library-store';
import { SettingsView } from './SettingsView';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

const clean = {
  files_checked: 120,
  bytes_checked: 4_200_000_000,
  problems: [],
  thumbnails_missing: 0,
  stray_folders: [],
};

/** A library is open, as the Library view leaves it. */
function openLibrary(over = {}) {
  state.details = libraryDetails(over);
  useLibraryStore.setState({ phase: 'ready', library: state.details });
}

const verifyButton = () => screen.getByRole('button', { name: 'Verify library' });

const finish = (result: Record<string, unknown>) =>
  act(() =>
    listeners.done.forEach((cb) =>
      cb({ id: 'job-verify-1', result: { kind: 'Verified', ...clean, ...result } as never }),
    ),
  );

/** Renders Settings and starts a check; it is running afterwards. */
async function start() {
  const user = userEvent.setup();
  render(<SettingsView />);
  await screen.findByRole('button', { name: 'Verify library' });
  await user.click(verifyButton());
  await screen.findByRole('progressbar', { name: 'Verify library' });
  return user;
}

describe('verifying the library from Settings', () => {
  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    useUiStore.setState({ view: 'settings', developFrame: null });
  });

  describe('what it says before a check', () => {
    it('says the library was never verified, and that verifying changes nothing', async () => {
      openLibrary();
      render(<SettingsView />);
      expect(await screen.findByText('Never verified')).toBeInTheDocument();
      expect(screen.getByText(/only reports what it finds: nothing is changed or deleted/)).toBe(
        screen.getByText(/works in a read-only library/),
      );
      expect(verifyButton()).toBeEnabled();
    });

    it('shows when it was last verified and how many problems it found', async () => {
      openLibrary({ last_verify_at: '2026-10-30T10:00:00Z', last_verify_problems: 2 });
      state.details = libraryDetails({
        last_verify_at: '2026-10-30T10:00:00Z',
        last_verify_problems: 2,
      });
      render(<SettingsView />);
      expect(await screen.findByText(/Oct 30, 2026.*2 problems/)).toBeInTheDocument();
    });

    it('says no problems when the last check found none', async () => {
      openLibrary({ last_verify_at: '2026-10-30T10:00:00Z', last_verify_problems: 0 });
      render(<SettingsView />);
      expect(await screen.findByText(/Oct 30, 2026.*no problems/)).toBeInTheDocument();
    });

    it('works in a read-only library', async () => {
      openLibrary({ read_only: true });
      render(<SettingsView />);
      expect(await screen.findByRole('button', { name: 'Verify library' })).toBeEnabled();
    });
  });

  describe('while it runs', () => {
    it('starts the job, shows its progress and turns the button off', async () => {
      openLibrary();
      await start();
      expect(ipc.libraryVerify).toHaveBeenCalledTimes(1);
      expect(verifyButton()).toBeDisabled();
      act(() =>
        listeners.progress.forEach((cb) =>
          cb({ id: 'job-verify-1', stage: 'verify', fraction: 0.4 }),
        ),
      );
      expect(screen.getByRole('progressbar', { name: 'Verify library' })).toHaveAttribute(
        'value',
        '40',
      );
    });

    it('cancels the job, and says nothing was recorded', async () => {
      openLibrary();
      const user = await start();
      await user.click(screen.getByRole('button', { name: 'Cancel' }));
      expect(ipc.jobCancel).toHaveBeenCalledWith('job-verify-1');
      act(() =>
        listeners.error.forEach((cb) =>
          cb({ id: 'job-verify-1', error: { code: 'Cancelled', message: 'Cancelled.' } as never }),
        ),
      );
      expect(
        await screen.findByText(/The check was cancelled; nothing was recorded/),
      ).toBeVisible();
      expect(verifyButton()).toBeEnabled();
    });

    it('keeps showing the check when Settings is left and opened again', async () => {
      openLibrary();
      await start();
      const again = render(<SettingsView />);
      expect(await within(again.container).findByRole('progressbar')).toBeInTheDocument();
      expect(ipc.libraryVerify).toHaveBeenCalledTimes(1);
    });

    it('says so when the check could not be started', async () => {
      openLibrary();
      state.verifyRefused = 'No library is open.';
      const user = userEvent.setup();
      render(<SettingsView />);
      await user.click(await screen.findByRole('button', { name: 'Verify library' }));
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The check could not be started: No library is open.',
      );
      expect(verifyButton()).toBeEnabled();
    });

    it('shows the message of a job that failed', async () => {
      openLibrary();
      await start();
      act(() =>
        listeners.error.forEach((cb) =>
          cb({
            id: 'job-verify-1',
            error: { code: 'Internal', message: 'The disk went away.' } as never,
          }),
        ),
      );
      expect(await screen.findByText(/The disk went away\./)).toBeInTheDocument();
      expect(verifyButton()).toBeEnabled();
    });
  });

  describe('what a check found', () => {
    it('says how much was checked and that nothing was found', async () => {
      openLibrary();
      await start();
      state.details = libraryDetails({
        last_verify_at: '2026-10-31T09:30:00Z',
        last_verify_problems: 0,
      });
      finish({});
      const result = await screen.findByText('Checked 120 files (4.2 GB).');
      expect(result.closest('[role="status"]')).toHaveTextContent('Nothing was found');
      expect(verifyButton()).toBeEnabled();
      // The figures are read again: "last verified" now shows this check.
      expect(await screen.findByText(/Oct 31, 2026.*no problems/)).toBeInTheDocument();
    });

    it('lists each problem with its file and the reason in words', async () => {
      openLibrary();
      await start();
      const problem = (reason: string, name: string) => ({
        frame_id: `frame-${name}`,
        roll_id: 'roll-1',
        role: 'Rgb',
        rel_path: `originals/roll-1/${name}/scan.tif`,
        reason,
        details: null,
      });
      finish({
        files_checked: 4,
        bytes_checked: null,
        problems: [
          problem('Missing', 'a'),
          problem('Changed', 'b'),
          problem('WrongSize', 'c'),
          problem('Unreadable', 'd'),
        ],
      });
      const status = (await screen.findByText('Checked 4 files.')).closest(
        '[role="status"]',
      ) as HTMLElement;
      expect(within(status).getByText('4 originals have a problem')).toBeInTheDocument();
      const items = within(within(status).getByRole('list')).getAllByRole('listitem');
      expect(items.map((li) => li.textContent)).toEqual([
        'originals/roll-1/a/scan.tif: missing',
        'originals/roll-1/b/scan.tif: changed since it was imported',
        'originals/roll-1/c/scan.tif: wrong size',
        'originals/roll-1/d/scan.tif: could not be read',
      ]);
      expect(within(status).queryByText(/Nothing was found/)).not.toBeInTheDocument();
    });

    it('reports missing thumbnails and stray folders without calling them problems', async () => {
      openLibrary();
      await start();
      finish({ thumbnails_missing: 3, stray_folders: ['originals/crashed-roll'] });
      const status = (await screen.findByText(/Checked 120 files/)).closest(
        '[role="status"]',
      ) as HTMLElement;
      expect(within(status).getByText('Every original is there and unchanged.')).toBeVisible();
      expect(
        within(status).getByText(/3 thumbnails are missing; they are made again/),
      ).toBeVisible();
      expect(
        within(status).getByText('Folders that belong to no roll or frame (never deleted)'),
      ).toBeVisible();
      expect(within(status).getByText('originals/crashed-roll')).toBeVisible();
      expect(within(status).queryByText(/Nothing was found/)).not.toBeInTheDocument();
    });

    it('has no accessibility violations: idle, running and with a result', async () => {
      openLibrary();
      const view = render(<SettingsView />);
      await screen.findByRole('button', { name: 'Verify library' });
      await expectNoA11yViolations(view.container);
      await userEvent.setup().click(verifyButton());
      await screen.findByRole('progressbar', { name: 'Verify library' });
      await expectNoA11yViolations(view.container);
      finish({
        problems: [
          {
            frame_id: 'f1',
            roll_id: 'r1',
            role: 'Rgb',
            rel_path: 'originals/r1/f1/scan.tif',
            reason: 'Missing',
            details: null,
          },
        ],
        thumbnails_missing: 1,
        stray_folders: ['originals/x'],
      });
      await waitFor(() => expect(screen.getByText(/1 original has a problem/)).toBeVisible());
      await expectNoA11yViolations(view.container);
    });
  });
});
