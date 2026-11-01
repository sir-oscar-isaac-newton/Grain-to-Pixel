import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations, mockLayout } from '../../test-support';
import { ipc, libraryDetails, makeFrames, makeRoll, resetFake, state } from './fake-ipc';
import { resetLibraryStore, useLibraryStore } from './library-store';
import { LibraryView } from './LibraryView';
import { resetThumbnails } from './thumbnails';

vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);

function openLibrary(over = {}) {
  state.details = libraryDetails(over);
  state.rolls = [makeRoll({ id: 'summer', name: 'Summer' })];
  state.frames = makeFrames('summer', 3);
}

const applyButton = () => screen.getByRole('button', { name: 'Apply settings…' });
const dialog = () => screen.getByRole('dialog', { name: 'Apply settings' });
const select = (...ids: string[]) => act(() => useLibraryStore.getState().setSelected(ids));

/** Opens the library, selects frames 1 and 2 and opens the dialog; the roll has loaded. */
async function openDialog(ids = ['summer-f00001', 'summer-f00002']) {
  const user = userEvent.setup();
  openLibrary();
  render(<LibraryView />);
  await screen.findAllByRole('gridcell');
  select(...ids);
  await user.click(applyButton());
  await screen.findByRole('dialog', { name: 'Apply settings' });
  await within(dialog()).findByRole('radio', { name: /All other frames of Summer/ });
  return user;
}

describe('ApplySettingsDialog', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    restoreLayout = mockLayout(900, 600);
  });
  afterEach(() => restoreLayout());

  describe('the button in the Library', () => {
    it('needs at least two frames selected: one to copy from and one to copy to', async () => {
      openLibrary();
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      expect(applyButton()).toBeDisabled();
      select('summer-f00001');
      expect(applyButton()).toBeDisabled();
      select('summer-f00001', 'summer-f00002');
      expect(applyButton()).toBeEnabled();
    });

    it('is out of reach in a library that cannot be changed', async () => {
      openLibrary({ read_only: true });
      render(<LibraryView />);
      await screen.findAllByRole('gridcell');
      select('summer-f00001', 'summer-f00002');
      expect(applyButton()).toBeDisabled();
    });
  });

  describe('the choices', () => {
    it('offers the selected frames as the source, with their numbers and roll', async () => {
      await openDialog(['summer-f00002', 'summer-f00001', 'summer-f00003']);
      const d = within(dialog());
      const from = d.getByRole('combobox', { name: 'Copy from' });
      expect(from).toHaveValue('summer-f00002');
      expect(
        within(from)
          .getAllByRole('option')
          .map((o) => o.textContent),
      ).toEqual(['Frame 2 of Summer', 'Frame 1 of Summer', 'Frame 3 of Summer']);
    });

    it('starts with colour and tone ticked and the rest not', async () => {
      await openDialog();
      const d = within(dialog());
      for (const [name, checked] of [
        ['Colour', true],
        ['Tone and curves', true],
        ['Crop and geometry', false],
        ['Film base', false],
        ['Levels', false],
        ['Dust removal', false],
      ] as const) {
        const box = d.getByRole('checkbox', { name: new RegExp(`^${name}`) });
        if (checked) expect(box).toBeChecked();
        else expect(box).not.toBeChecked();
      }
    });

    it('offers the output sharpening too, unticked, and copies it in the order of the groups', async () => {
      const user = await openDialog();
      const d = within(dialog());
      expect(d.getAllByRole('checkbox')).toHaveLength(7);
      const output = d.getByRole('checkbox', { name: /^Output sharpening/ });
      expect(output).not.toBeChecked();
      expect(
        d.getByText('The amount and radius of the sharpening applied when exporting.'),
      ).toBeInTheDocument();
      // It is the last of the groups.
      expect(d.getAllByRole('checkbox').at(-1)).toBe(output);

      await user.click(output);
      await user.click(
        d.getByRole('button', { name: 'Apply colour, tone, and output sharpening to 1 frame' }),
      );
      await waitFor(() => expect(state.applies).toHaveLength(1));
      expect(state.applies[0]?.fields).toEqual(['Color', 'Tone', 'Output']);
    });

    it('counts the frames it would change, and defaults to the selection', async () => {
      await openDialog();
      const d = within(dialog());
      expect(d.getByRole('radio', { name: 'The other selected frame (1)' })).toBeChecked();
      expect(d.getByRole('radio', { name: 'All other frames of Summer (2)' })).not.toBeChecked();
      expect(d.getByRole('button', { name: 'Apply colour and tone to 1 frame' })).toBeEnabled();
      expect(
        d.getByText(
          'This replaces the colour and tone settings on 1 frame, whatever they were. It cannot be undone.',
        ),
      ).toBeInTheDocument();
    });

    it('follows the source: the others are whichever frames are not it', async () => {
      const user = await openDialog(['summer-f00001', 'summer-f00002', 'summer-f00003']);
      const d = within(dialog());
      expect(d.getByRole('radio', { name: 'The other selected frames (2)' })).toBeChecked();
      await user.selectOptions(d.getByRole('combobox', { name: 'Copy from' }), 'summer-f00003');
      await user.click(d.getByRole('button', { name: 'Apply colour and tone to 2 frames' }));
      await waitFor(() => expect(state.applies).toHaveLength(1));
      expect(state.applies[0]).toMatchObject({
        source: 'summer-f00003',
        frameIds: ['summer-f00001', 'summer-f00002'],
      });
    });

    it('cannot apply with no group chosen, and says so', async () => {
      const user = await openDialog();
      const d = within(dialog());
      await user.click(d.getByRole('checkbox', { name: /^Colour/ }));
      await user.click(d.getByRole('checkbox', { name: /^Tone and curves/ }));
      expect(d.getByRole('button', { name: 'Apply' })).toBeDisabled();
      expect(d.getByText('Choose at least one group of settings.')).toBeInTheDocument();
    });
  });

  describe('applying', () => {
    it('copies the chosen groups to the other selected frames, and says it is done', async () => {
      const user = await openDialog();
      const d = within(dialog());
      await user.click(d.getByRole('button', { name: 'Apply colour and tone to 1 frame' }));
      await waitFor(() => expect(state.applies).toHaveLength(1));
      expect(state.applies[0]).toEqual({
        source: 'summer-f00001',
        frameIds: ['summer-f00002'],
        fields: ['Color', 'Tone'],
      });
      expect(await d.findByText('Applied to 1 frame.')).toBeInTheDocument();
      await user.click(d.getByRole('button', { name: 'Close' }));
      expect(screen.queryByRole('dialog', { name: 'Apply settings' })).not.toBeInTheDocument();
    });

    it('copies to the whole roll, in the order of the groups, whatever order they were ticked', async () => {
      const user = await openDialog();
      const d = within(dialog());
      await user.click(d.getByRole('checkbox', { name: /^Film base/ }));
      await user.click(d.getByRole('checkbox', { name: /^Crop and geometry/ }));
      await user.click(d.getByRole('radio', { name: 'All other frames of Summer (2)' }));
      await user.click(
        d.getByRole('button', { name: 'Apply colour, tone, geometry, and film base to 2 frames' }),
      );
      await waitFor(() => expect(state.applies).toHaveLength(1));
      expect(state.applies[0]).toEqual({
        source: 'summer-f00001',
        frameIds: ['summer-f00002', 'summer-f00003'],
        fields: ['Color', 'Tone', 'Geometry', 'Base'],
      });
      expect(await d.findByText('Applied to 2 frames.')).toBeInTheDocument();
    });

    it('says why it could not, and stays open to try again', async () => {
      state.applyRefused = 'This frame does not exist.';
      const user = await openDialog();
      const d = within(dialog());
      await user.click(d.getByRole('button', { name: 'Apply colour and tone to 1 frame' }));
      expect(await d.findByRole('alert')).toHaveTextContent(
        'The settings could not be applied: This frame does not exist.',
      );
      expect(d.getByRole('button', { name: 'Apply colour and tone to 1 frame' })).toBeEnabled();
      expect(d.queryByText(/Applied to/)).not.toBeInTheDocument();
    });

    it('changes nothing when it is cancelled, or closed with Escape', async () => {
      const user = await openDialog();
      await user.click(within(dialog()).getByRole('button', { name: 'Cancel' }));
      expect(screen.queryByRole('dialog', { name: 'Apply settings' })).not.toBeInTheDocument();
      await user.click(applyButton());
      await screen.findByRole('dialog', { name: 'Apply settings' });
      await user.keyboard('{Escape}');
      await waitFor(() =>
        expect(screen.queryByRole('dialog', { name: 'Apply settings' })).not.toBeInTheDocument(),
      );
      expect(ipc.recipeApplyTo).not.toHaveBeenCalled();
    });
  });

  it('has no accessibility violations: the form and the result', async () => {
    const user = await openDialog();
    await expectNoA11yViolations(document.body);
    await user.click(
      within(dialog()).getByRole('button', { name: 'Apply colour and tone to 1 frame' }),
    );
    await within(dialog()).findByText('Applied to 1 frame.');
    await expectNoA11yViolations(document.body);
  });
});
