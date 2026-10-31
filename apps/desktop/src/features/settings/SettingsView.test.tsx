import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import { expectNoA11yViolations } from '../../test-support';
import { DEFAULT_FLAGS, ipc, libraryDetails, resetFake, state } from '../library/fake-ipc';
import { resetLibraryStore, useLibraryStore } from '../library/library-store';
import { SettingsView } from './SettingsView';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

/** A library is open, as the Library view leaves it. */
function openLibrary(over = {}) {
  state.details = libraryDetails(over);
  useLibraryStore.setState({ phase: 'ready', library: state.details });
}

const flag = (name: RegExp) => screen.getByRole('switch', { name });

describe('SettingsView', () => {
  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    useUiStore.setState({ view: 'settings', developFrame: null });
  });

  describe('the library', () => {
    it('shows where it is and how much room is left', async () => {
      openLibrary();
      render(<SettingsView />);
      await screen.findByRole('switch', { name: /Guided capture/ });
      expect(screen.getByText('Film')).toBeInTheDocument();
      expect(screen.getByText('/films/Library')).toBeInTheDocument();
      expect(screen.getByText('54.9 GB')).toBeInTheDocument();
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
    });

    it('asks again for the free space, which has changed since the library was opened', async () => {
      openLibrary();
      state.details = libraryDetails({ free_bytes: 12_300_000_000 });
      render(<SettingsView />);
      expect(await screen.findByText('12.3 GB')).toBeInTheDocument();
      expect(ipc.libraryInfo).toHaveBeenCalled();
    });

    it('says when the free space is not known', () => {
      openLibrary({ free_bytes: null });
      state.details = libraryDetails({ free_bytes: null });
      render(<SettingsView />);
      expect(screen.getByText('Not known')).toBeInTheDocument();
    });

    it('says when it can be looked at but not changed', async () => {
      openLibrary({ read_only: true });
      render(<SettingsView />);
      await screen.findByRole('switch', { name: /Guided capture/ });
      expect(screen.getByRole('status')).toHaveTextContent(
        'made by a newer version of the app, so it can be looked at but not changed',
      );
    });

    it('says when none is open, and goes to the Library view', async () => {
      const user = userEvent.setup();
      render(<SettingsView />);
      expect(screen.getByText('No library is open.')).toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Open the Library view' }));
      expect(useUiStore.getState().view).toBe('library');
    });
  });

  describe('the feature flags', () => {
    it('lists the five flags with what they do and their defaults', async () => {
      openLibrary();
      render(<SettingsView />);
      const group = await screen.findByRole('group', { name: 'Feature flags' });
      const switches = within(group).getAllByRole('switch');
      expect(switches.map((s) => s.closest('label')?.textContent)).toEqual([
        expect.stringContaining('Infrared dust removal'),
        expect.stringContaining('RGB triplet import'),
        expect.stringContaining('Guided capture'),
        expect.stringContaining('Developer overlay'),
        expect.stringContaining('Experimental auto levels'),
      ]);
      expect(switches.map((s) => (s as HTMLInputElement).checked)).toEqual([
        true,
        true,
        true,
        false,
        false,
      ]);
      expect(
        within(flag(/Developer overlay/).closest('label') as HTMLElement).getByText(
          /Off by default\./,
        ),
      ).toBeInTheDocument();
      expect(
        within(flag(/Infrared dust removal/).closest('label') as HTMLElement).getByText(
          /On by default\./,
        ),
      ).toBeInTheDocument();
      expect(screen.queryByText('(not the default)')).not.toBeInTheDocument();
    });

    it('shows the flags as they were saved', async () => {
      openLibrary();
      state.flags = { ...DEFAULT_FLAGS, guided_capture: false, dev_overlay: true };
      render(<SettingsView />);
      await screen.findByRole('switch', { name: /Guided capture/ });
      expect(flag(/Guided capture/)).not.toBeChecked();
      expect(flag(/Developer overlay/)).toBeChecked();
      expect(screen.getAllByText('(not the default)')).toHaveLength(2);
    });

    it('saves a flag the moment it is switched, and marks it when it is not the default', async () => {
      openLibrary();
      const user = userEvent.setup();
      render(<SettingsView />);
      await user.click(await screen.findByRole('switch', { name: /Developer overlay/ }));
      await waitFor(() => expect(flag(/Developer overlay/)).toBeChecked());
      expect(state.settingsPatches).toEqual([{ flags: { dev_overlay: true } }]);
      expect(state.flags.dev_overlay).toBe(true);
      expect(await screen.findByText('(not the default)')).toBeInTheDocument();

      await user.click(flag(/Infrared dust removal/));
      await waitFor(() => expect(flag(/Infrared dust removal/)).not.toBeChecked());
      expect(state.settingsPatches[1]).toEqual({ flags: { dust_removal: false } });
      expect(screen.getAllByText('(not the default)')).toHaveLength(2);
    });

    it('switches a flag back and says why, when it could not be saved', async () => {
      openLibrary();
      state.settingsRefused = 'The disk is full.';
      const user = userEvent.setup();
      render(<SettingsView />);
      await user.click(await screen.findByRole('switch', { name: /Experimental auto levels/ }));
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The setting could not be saved: The disk is full.',
      );
      expect(flag(/Experimental auto levels/)).not.toBeChecked();
      expect(flag(/Experimental auto levels/)).toBeEnabled();
      expect(state.flags.experimental_auto_levels).toBe(false);
    });

    it('resets every flag that is not at its default, and only those', async () => {
      openLibrary();
      state.flags = { ...DEFAULT_FLAGS, triplet_import: false, dev_overlay: true };
      const user = userEvent.setup();
      render(<SettingsView />);
      await screen.findByRole('switch', { name: /RGB triplet import/ });
      await user.click(screen.getByRole('button', { name: 'Reset flags to their defaults' }));
      await waitFor(() => expect(flag(/RGB triplet import/)).toBeChecked());
      expect(state.settingsPatches).toEqual([
        { flags: { triplet_import: true, dev_overlay: false } },
      ]);
      expect(flag(/Developer overlay/)).not.toBeChecked();
      expect(screen.getByRole('button', { name: 'Reset flags to their defaults' })).toBeDisabled();
    });

    it('has nothing to reset at the defaults', async () => {
      openLibrary();
      render(<SettingsView />);
      await screen.findByRole('switch', { name: /Guided capture/ });
      expect(screen.getByRole('button', { name: 'Reset flags to their defaults' })).toBeDisabled();
    });

    it('says when the settings cannot be read, and offers no switches', async () => {
      openLibrary();
      state.settingsUnreadable = 'The settings file is damaged.';
      render(<SettingsView />);
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The settings could not be read: The settings file is damaged.',
      );
      expect(screen.queryByRole('switch')).not.toBeInTheDocument();
    });
  });

  it('has no accessibility violations: with a library, without one, and with an error', async () => {
    openLibrary();
    const withLibrary = render(<SettingsView />);
    await screen.findByRole('switch', { name: /Guided capture/ });
    await expectNoA11yViolations(withLibrary.container);
    withLibrary.unmount();

    resetLibraryStore();
    state.details = null;
    state.settingsUnreadable = 'The settings file is damaged.';
    const without = render(<SettingsView />);
    await screen.findByRole('alert');
    await expectNoA11yViolations(without.container);
  });
});
