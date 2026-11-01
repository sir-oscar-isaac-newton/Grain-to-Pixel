import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import { expectNoA11yViolations } from '../../test-support';
import { libraryDetails, resetFake, state } from '../library/fake-ipc';
import { resetLibraryStore, useLibraryStore } from '../library/library-store';
import { SettingsView } from './SettingsView';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

async function openSettings() {
  state.details = libraryDetails();
  useLibraryStore.setState({ phase: 'ready', library: state.details });
  const user = userEvent.setup();
  render(<SettingsView />);
  const section = within(await screen.findByRole('region', { name: 'Keyboard' }));
  return { user, section };
}

const change = (action: string) =>
  screen.getByRole('button', { name: `Change the key for ${action}` });
const keyOf = (action: string) => change(action).closest('li')?.querySelector('kbd')?.textContent;

describe('the Keyboard section of Settings', () => {
  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    useUiStore.setState({ view: 'settings', developFrame: null });
  });

  it('lists every shortcut with its key, the colour keys apart from the rest', async () => {
    const { section } = await openSettings();
    expect(section.getAllByRole('listitem')).toHaveLength(14);
    expect(section.getByRole('heading', { name: 'Develop view' })).toBeInTheDocument();
    expect(section.getByRole('heading', { name: 'Colour' })).toBeInTheDocument();
    expect(keyOf('Add red')).toBe('Q');
    expect(keyOf('Add cyan')).toBe('A');
    expect(keyOf('Add yellow')).toBe('D');
    expect(keyOf('Back to the Library (first stops picking)')).toBe('Esc');
    expect(keyOf('Next frame')).toBe(']');
    expect(
      section.getByText(/A press moves a colour slider by 10 steps, Shift and the key by 1/),
    ).toBeInTheDocument();
    expect(section.queryByText('(not the default)')).not.toBeInTheDocument();
    expect(section.getByRole('button', { name: 'Reset keyboard to defaults' })).toBeDisabled();
  });

  it('shows the keys as they were saved, and marks those that are not the default', async () => {
    state.keys = { 'colour.red': 'r' };
    const { section } = await openSettings();
    await waitFor(() => expect(keyOf('Add red')).toBe('R'));
    expect(section.getAllByText('(not the default)')).toHaveLength(1);
  });

  describe('changing a key', () => {
    it('takes the next key pressed, and saves it at once', async () => {
      const { user, section } = await openSettings();
      await user.click(change('Add red'));
      expect(change('Add red')).toHaveAttribute('aria-pressed', 'true');
      expect(section.getByRole('status')).toHaveTextContent('Press the new key for “Add red”.');

      await user.keyboard('r');
      await waitFor(() => expect(keyOf('Add red')).toBe('R'));
      expect(state.settingsPatches).toEqual([{ keys: { 'colour.red': 'r' } }]);
      expect(state.keys).toEqual({ 'colour.red': 'r' });
      expect(section.queryByRole('status')).not.toBeInTheDocument();
      expect(change('Add red')).toHaveAttribute('aria-pressed', 'false');
      expect(section.getAllByText('(not the default)')).toHaveLength(1);
      expect(section.getByRole('button', { name: 'Reset keyboard to defaults' })).toBeEnabled();
    });

    it('stores a capital as the letter, since Shift does not change which action a key means', async () => {
      const { user } = await openSettings();
      await user.click(change('Fit to the window'));
      await user.keyboard('{Shift>}F{/Shift}');
      await waitFor(() => expect(state.keys).toEqual({ 'develop.fit': 'f' }));
    });

    it('keeps the other changes when another is made', async () => {
      state.keys = { 'colour.red': 'r' };
      const { user } = await openSettings();
      await waitFor(() => expect(keyOf('Add red')).toBe('R'));
      await user.click(change('Add blue'));
      await user.keyboard('b');
      await waitFor(() => expect(state.keys).toEqual({ 'colour.red': 'r', 'colour.blue': 'b' }));
    });

    it('refuses a key another action has, and says which', async () => {
      const { user, section } = await openSettings();
      await user.click(change('Add red'));
      await user.keyboard('a');
      expect(await section.findByRole('alert')).toHaveTextContent(
        'A is already used for “Add cyan”. Choose another key, or change that one first.',
      );
      expect(keyOf('Add red')).toBe('Q');
      expect(state.settingsPatches).toHaveLength(0);
      // Still waiting for a key, so a good one is taken.
      expect(change('Add red')).toHaveAttribute('aria-pressed', 'true');
      await user.keyboard('r');
      await waitFor(() => expect(keyOf('Add red')).toBe('R'));
      expect(section.queryByRole('alert')).not.toBeInTheDocument();
    });

    it('refuses the key it already has no more than a clash, but accepts it again quietly', async () => {
      const { user } = await openSettings();
      await user.click(change('Add red'));
      await user.keyboard('q');
      await waitFor(() => expect(state.settingsPatches).toHaveLength(1));
      // Nothing differs from the defaults, so nothing is kept.
      expect(state.keys).toEqual({});
    });

    it.each([
      ['a combination with Ctrl', '{Control>}r{/Control}', /without Ctrl, Alt or Command/],
      ['a combination with Alt', '{Alt>}r{/Alt}', /without Ctrl, Alt or Command/],
      ['the space bar', ' ', /cannot be a shortcut/],
      ['Enter', '{Enter}', /cannot be a shortcut/],
      ['an arrow key', '{ArrowLeft}', /cannot be a shortcut/],
    ])('refuses %s with a reason', async (_name, keys, message) => {
      const { user, section } = await openSettings();
      await user.click(change('Add red'));
      await user.keyboard(keys);
      expect(await section.findByRole('alert')).toHaveTextContent(message);
      expect(state.settingsPatches).toHaveLength(0);
    });

    it('waits through a modifier pressed on its own', async () => {
      const { user, section } = await openSettings();
      await user.click(change('Add red'));
      await user.keyboard('{Shift}');
      expect(section.queryByRole('alert')).not.toBeInTheDocument();
      expect(change('Add red')).toHaveAttribute('aria-pressed', 'true');
    });

    it('is cancelled by Escape, which changes nothing', async () => {
      const { user, section } = await openSettings();
      await user.click(change('Add red'));
      await user.keyboard('{Escape}');
      expect(section.queryByRole('status')).not.toBeInTheDocument();
      expect(change('Add red')).toHaveAttribute('aria-pressed', 'false');
      expect(state.settingsPatches).toHaveLength(0);
      expect(useUiStore.getState().view).toBe('settings');
    });

    it('is cancelled by choosing the same button again, and moves to another action', async () => {
      const { user, section } = await openSettings();
      await user.click(change('Add red'));
      await user.click(change('Add red'));
      expect(change('Add red')).toHaveAttribute('aria-pressed', 'false');
      await user.click(change('Add red'));
      await user.click(change('Add green'));
      expect(change('Add red')).toHaveAttribute('aria-pressed', 'false');
      expect(section.getByRole('status')).toHaveTextContent('Press the new key for “Add green”.');
    });

    it('switches back and says why when the key could not be saved', async () => {
      state.settingsRefused = 'The disk is full.';
      const { user } = await openSettings();
      await user.click(change('Add red'));
      await user.keyboard('r');
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The setting could not be saved: The disk is full.',
      );
      expect(keyOf('Add red')).toBe('Q');
    });
  });

  it('resets every key to its default', async () => {
    state.keys = { 'colour.red': 'r', 'develop.fit': 'f' };
    const { user, section } = await openSettings();
    await waitFor(() => expect(keyOf('Add red')).toBe('R'));
    await user.click(section.getByRole('button', { name: 'Reset keyboard to defaults' }));
    await waitFor(() => expect(keyOf('Add red')).toBe('Q'));
    expect(state.settingsPatches).toEqual([{ keys: {} }]);
    expect(keyOf('Fit to the window')).toBe('0');
    expect(section.getByRole('button', { name: 'Reset keyboard to defaults' })).toBeDisabled();
  });

  it('has no accessibility violations: at rest, picking a key, and with a message', async () => {
    const { user, section } = await openSettings();
    await expectNoA11yViolations(document.body);
    await user.click(change('Add red'));
    await expectNoA11yViolations(document.body);
    await user.keyboard('a');
    await section.findByRole('alert');
    await expectNoA11yViolations(document.body);
  });
});
