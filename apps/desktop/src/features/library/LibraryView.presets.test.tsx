import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DevelopPreset } from '../../shared/ipc';
import { expectNoA11yViolations, mockLayout } from '../../test-support';
import {
  ipc,
  libraryDetails,
  makeFrames,
  makeRecipe,
  makeRoll,
  resetFake,
  state,
} from './fake-ipc';
import { resetLibraryStore, useLibraryStore } from './library-store';
import { LibraryView } from './LibraryView';
import { resetThumbnails } from './thumbnails';

vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);

function openLibrary(over = {}) {
  state.details = libraryDetails(over);
  state.rolls = [makeRoll({ id: 'summer', name: 'Summer' })];
  state.frames = makeFrames('summer', 3);
}

const presetButton = () => screen.getByRole('button', { name: 'Apply preset…' });
const dialog = () => screen.getByRole('dialog', { name: 'Apply preset' });
const select = (...ids: string[]) => act(() => useLibraryStore.getState().setSelected(ids));

function preset(name: string, fields: DevelopPreset['fields']): DevelopPreset {
  return { name, fields, recipe: makeRecipe(name) as DevelopPreset['recipe'] };
}

async function openDialog(ids = ['summer-f00001', 'summer-f00002']) {
  const user = userEvent.setup();
  openLibrary();
  render(<LibraryView />);
  await screen.findAllByRole('gridcell');
  select(...ids);
  await user.click(presetButton());
  await screen.findByRole('dialog', { name: 'Apply preset' });
  return user;
}

describe('Apply preset in the Library', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    restoreLayout = mockLayout(900, 600);
  });
  afterEach(() => restoreLayout());

  it('needs a frame selected', async () => {
    openLibrary();
    render(<LibraryView />);
    await screen.findAllByRole('gridcell');
    expect(presetButton()).toBeDisabled();
    select('summer-f00001');
    expect(presetButton()).toBeEnabled();
  });

  it('is out of reach in a library that cannot be changed', async () => {
    openLibrary({ read_only: true });
    render(<LibraryView />);
    await screen.findAllByRole('gridcell');
    select('summer-f00001', 'summer-f00002');
    expect(presetButton()).toBeDisabled();
  });

  it('lists the presets with their parts and applies one to the selected frames', async () => {
    state.developPresets = [preset('Warm', ['Color', 'Tone']), preset('Crop', ['Geometry'])];
    const user = await openDialog();
    const d = within(dialog());
    expect(await d.findByText('Warm')).toBeInTheDocument();
    expect(d.getByText('colour and tone')).toBeInTheDocument();
    expect(d.getByText('geometry')).toBeInTheDocument();
    expect(d.queryByRole('button', { name: /^Rename/ })).not.toBeInTheDocument();
    expect(d.queryByRole('button', { name: /^Delete/ })).not.toBeInTheDocument();
    expect(d.queryByRole('button', { name: /^Save/ })).not.toBeInTheDocument();

    await user.click(d.getByRole('button', { name: 'Apply the preset Warm' }));
    expect(await d.findByRole('status')).toHaveTextContent('Applied “Warm” to 2 frames.');
    expect(state.valueApplies).toEqual([
      {
        frameIds: ['summer-f00001', 'summer-f00002'],
        recipe: state.developPresets[0]?.recipe,
        fields: ['Color', 'Tone'],
      },
    ]);
    await user.click(d.getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('dialog', { name: 'Apply preset' })).not.toBeInTheDocument();
  });

  it('says so when the preset could not be applied', async () => {
    state.developPresets = [preset('Warm', ['Tone'])];
    state.applyRefused = 'Nothing was changed.';
    const user = await openDialog(['summer-f00001']);
    const d = within(dialog());
    await user.click(await d.findByRole('button', { name: 'Apply the preset Warm' }));
    expect(await d.findByRole('alert')).toHaveTextContent(
      'The preset could not be applied: Nothing was changed.',
    );
    expect(d.queryByRole('status')).not.toBeInTheDocument();
  });

  it('points to the Develop view when there are no presets', async () => {
    await openDialog();
    const d = within(dialog());
    expect(
      await d.findByText('There are no presets yet. Create them in the Develop view.'),
    ).toBeInTheDocument();
    expect(d.queryByRole('button', { name: /^Apply the preset/ })).not.toBeInTheDocument();
    await waitFor(() => expect(ipc.settingsGet).toHaveBeenCalled());
  });

  it('has no accessibility violations', async () => {
    state.developPresets = [preset('Warm', ['Color', 'Tone'])];
    await openDialog();
    await within(dialog()).findByText('Warm');
    await expectNoA11yViolations(dialog());
  });
});
