import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
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
} from '../library/fake-ipc';
import { resetLibraryStore, useLibraryStore } from '../library/library-store';
import { resetThumbnails } from '../library/thumbnails';
import { DevelopView } from './DevelopView';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

function openLibrary(over = {}) {
  state.details = libraryDetails(over);
  state.rolls = [makeRoll({ id: 'summer', name: 'Summer' })];
  state.frames = makeFrames('summer', 5);
  useLibraryStore.setState({
    phase: 'ready',
    library: state.details,
    rolls: state.rolls,
    rollId: 'summer',
  });
}

const canvas = () =>
  waitFor(() => {
    if (!document.querySelector('canvas[role="img"]')) throw new Error('no picture yet');
  });

const button = (name: string | RegExp) => screen.getByRole('button', { name });
const slider = (name: string) => screen.getByRole('slider', { name });
const value = (name: string) => Number(slider(name).getAttribute('aria-valuenow'));
const dialog = () => screen.getByRole('dialog', { name: 'Develop presets' });
const inDialog = () => within(dialog());
const lastPresets = () => state.settingsPatches.at(-1)?.develop_presets as DevelopPreset[];

/** A preset holding a stand-in recipe with a tone and a different crop angle. */
function preset(name: string, fields: DevelopPreset['fields'] = ['Tone']): DevelopPreset {
  return {
    name,
    fields,
    recipe: makeRecipe('preset', {
      tone: { contrast: 25 },
      geometry: {
        crop: { x: 0, y: 0, w: 1, h: 1 },
        angle_deg: 4,
        rot90: 0,
        flip_h: false,
        flip_v: false,
      },
    }) as DevelopPreset['recipe'],
  };
}

describe('Develop presets', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    restoreLayout = mockLayout(900, 600);
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => null);
  });
  afterEach(() => {
    restoreLayout();
    vi.restoreAllMocks();
  });

  async function openFrame(over = {}) {
    const user = userEvent.setup();
    openLibrary(over);
    useUiStore.setState({ view: 'develop', developFrame: { id: 'summer-f00001', index: 0 } });
    const view = render(<DevelopView />);
    await canvas();
    await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalled());
    // A library that cannot be changed leaves the edit controls off, Reset all included.
    if (!('read_only' in over)) await waitFor(() => expect(button('Reset all')).toBeEnabled());
    return { user, ...view };
  }

  async function openPresets(over = {}) {
    const frame = await openFrame(over);
    await frame.user.click(button('Presets…'));
    await screen.findByRole('dialog', { name: 'Develop presets' });
    return frame.user;
  }

  describe('saving the current edit', () => {
    it('ticks colour, the colour mixer, split toning and tone and leaves the rest, and saves the edit', async () => {
      const user = await openPresets();
      const d = inDialog();
      expect(
        await d.findByText(
          'There are no presets yet. Save the current edit as a preset to make one.',
        ),
      ).toBeInTheDocument();
      await user.click(d.getByRole('button', { name: 'Save current edit as preset…' }));

      for (const [part, ticked] of [
        ['Colour', true],
        ['Colour mixer', true],
        ['Split toning', true],
        ['Tone and curves', true],
        ['Crop and geometry', false],
        ['Film base', false],
        ['Levels', false],
        ['Dust removal', false],
        ['Noise reduction', false],
        ['Output sharpening', false],
      ] as const) {
        const box = d.getByRole('checkbox', { name: part });
        if (ticked) expect(box).toBeChecked();
        else expect(box).not.toBeChecked();
      }

      await user.type(d.getByRole('textbox', { name: 'Preset name' }), '  Warm  ');
      await user.click(d.getByRole('button', { name: 'Save preset' }));

      await waitFor(() => expect(lastPresets()).toHaveLength(1));
      expect(lastPresets()[0]).toMatchObject({
        name: 'Warm',
        fields: ['Color', 'Mixer', 'SplitTone', 'Tone'],
        recipe: { tag: 'saved' },
      });
      expect(await d.findByText('Warm')).toBeInTheDocument();
      expect(d.getByText('colour, colour mixer, split toning, and tone')).toBeInTheDocument();
      expect(
        d.queryByRole('textbox', { name: 'Preset name' }),
        'the form is closed once saved',
      ).not.toBeInTheDocument();
    });

    it('saves the parts that are ticked, in the order they are listed', async () => {
      const user = await openPresets();
      const d = inDialog();
      await user.click(d.getByRole('button', { name: 'Save current edit as preset…' }));
      await user.type(d.getByRole('textbox', { name: 'Preset name' }), 'Crop only');
      for (const part of ['Colour', 'Colour mixer', 'Split toning', 'Tone and curves']) {
        await user.click(d.getByRole('checkbox', { name: part }));
      }
      await user.click(d.getByRole('checkbox', { name: 'Output sharpening' }));
      await user.click(d.getByRole('checkbox', { name: 'Crop and geometry' }));
      await user.click(d.getByRole('button', { name: 'Save preset' }));
      await waitFor(() => expect(lastPresets()).toHaveLength(1));
      expect(lastPresets()[0]?.fields).toEqual(['Geometry', 'Output']);
    });

    it('needs a name that is not empty or in use, and at least one part', async () => {
      state.developPresets = [preset('Warm')];
      const user = await openPresets();
      const d = inDialog();
      await d.findByText('Warm');
      await user.click(d.getByRole('button', { name: 'Save current edit as preset…' }));
      const save = d.getByRole('button', { name: 'Save preset' });
      const name = d.getByRole('textbox', { name: 'Preset name' });
      expect(save).toBeDisabled();

      await user.type(name, '   ');
      expect(save).toBeDisabled();
      expect(d.getByRole('alert')).toHaveTextContent('Type a name for the preset.');

      await user.clear(name);
      await user.type(name, ' WARM ');
      expect(save).toBeDisabled();
      expect(d.getByRole('alert')).toHaveTextContent('A preset with that name already exists.');

      await user.clear(name);
      await user.type(name, 'Cool');
      expect(save).toBeEnabled();
      expect(d.queryByRole('alert')).not.toBeInTheDocument();

      for (const part of ['Colour', 'Colour mixer', 'Split toning', 'Tone and curves']) {
        await user.click(d.getByRole('checkbox', { name: part }));
      }
      expect(save).toBeDisabled();
      expect(d.getByText('Choose at least one group of settings.')).toBeInTheDocument();
      expect(state.settingsPatches).toHaveLength(0);
    });

    it('says so when the presets could not be saved', async () => {
      state.settingsRefused = 'The disk is full.';
      const user = await openPresets();
      const d = inDialog();
      await user.click(d.getByRole('button', { name: 'Save current edit as preset…' }));
      await user.type(d.getByRole('textbox', { name: 'Preset name' }), 'Warm');
      await user.click(d.getByRole('button', { name: 'Save preset' }));
      expect(await d.findByRole('alert')).toHaveTextContent(
        'The presets could not be saved: The disk is full.',
      );
      expect(d.queryByText('Warm', { selector: 'strong' })).not.toBeInTheDocument();
    });
  });

  describe('changing a preset', () => {
    it('updates a preset with the current edit, keeping its name and parts', async () => {
      state.developPresets = [preset('Warm', ['Color', 'Tone'])];
      const user = await openPresets();
      const d = inDialog();
      await user.click(
        await d.findByRole('button', { name: 'Update the preset Warm with the current edit' }),
      );
      await waitFor(() => expect(lastPresets()).toHaveLength(1));
      expect(lastPresets()[0]).toMatchObject({
        name: 'Warm',
        fields: ['Color', 'Tone'],
        recipe: { tag: 'saved' },
      });
    });

    it('renames a preset with the checks of a new name, and may change only its case', async () => {
      state.developPresets = [preset('Warm'), preset('Cool')];
      const user = await openPresets();
      const d = inDialog();
      await user.click(await d.findByRole('button', { name: 'Rename the preset Warm' }));
      const name = d.getByRole('textbox', { name: 'Preset name' });
      expect(name).toHaveValue('Warm');
      const save = d.getByRole('button', { name: 'Rename preset' });

      await user.clear(name);
      expect(save).toBeDisabled();
      await user.type(name, 'cool');
      expect(save).toBeDisabled();
      expect(d.getByRole('alert')).toHaveTextContent('A preset with that name already exists.');

      await user.clear(name);
      await user.type(name, 'warm');
      expect(save).toBeEnabled();
      await user.clear(name);
      await user.type(name, ' Golden ');
      await user.click(save);
      await waitFor(() => expect(lastPresets().map((p) => p.name)).toEqual(['Golden', 'Cool']));
      expect(lastPresets()[0]?.fields).toEqual(['Tone']);
      expect(d.queryByRole('textbox', { name: 'Preset name' })).not.toBeInTheDocument();
    });

    it('deletes a preset only after asking', async () => {
      state.developPresets = [preset('Warm'), preset('Cool')];
      const user = await openPresets();
      const d = inDialog();
      await user.click(await d.findByRole('button', { name: 'Delete the preset Warm' }));
      const ask = d.getByRole('group', { name: 'Delete preset' });
      expect(ask).toHaveTextContent('Delete the preset “Warm”? This cannot be undone.');
      expect(state.settingsPatches).toHaveLength(0);

      await user.click(within(ask).getByRole('button', { name: 'Keep it' }));
      expect(d.queryByRole('group', { name: 'Delete preset' })).not.toBeInTheDocument();
      expect(state.settingsPatches).toHaveLength(0);

      await user.click(d.getByRole('button', { name: 'Delete the preset Warm' }));
      await user.click(
        within(d.getByRole('group', { name: 'Delete preset' })).getByRole('button', {
          name: 'Delete preset',
        }),
      );
      await waitFor(() => expect(lastPresets().map((p) => p.name)).toEqual(['Cool']));
      expect(d.queryByText('Warm', { selector: 'strong' })).not.toBeInTheDocument();
    });
  });

  describe('applying a preset', () => {
    it('changes only the preset parts, as one undo step that undo restores', async () => {
      state.developPresets = [preset('Contrasty', ['Tone'])];
      const { user } = await openFrame();
      expect(value('Contrast')).toBe(0);
      expect(button('Undo')).toBeDisabled();
      await user.click(button('Presets…'));
      await user.click(
        await inDialog().findByRole('button', { name: 'Apply the preset Contrasty' }),
      );
      expect(await inDialog().findByRole('status')).toHaveTextContent(
        'Applied the preset “Contrasty” to this frame.',
      );
      await user.click(inDialog().getByRole('button', { name: 'Close' }));

      expect(value('Contrast')).toBe(25);
      expect(ipc.recipeMergeFields).toHaveBeenCalledTimes(1);
      const lastRender = ipc.developRender.mock.calls.at(-1)?.[1] as never;
      expect(lastRender).toMatchObject({
        tone: { contrast: 25 },
        geometry: { angle_deg: 1.5 },
      });
      await waitFor(
        () => expect(state.saved.at(-1)?.recipe).toMatchObject({ tone: { contrast: 25 } }),
        { timeout: 3000 },
      );

      await user.click(button('Undo'));
      await waitFor(() => expect(value('Contrast')).toBe(0));
      expect(button('Undo'), 'it was one step').toBeDisabled();
      expect(button('Redo')).toBeEnabled();
    });

    it('is out of reach in a library that cannot be changed, while presets can still be saved', async () => {
      state.developPresets = [preset('Contrasty')];
      const user = await openPresets({ read_only: true });
      const d = inDialog();
      expect(await d.findByRole('button', { name: 'Apply the preset Contrasty' })).toBeDisabled();
      expect(d.getByText(/cannot be applied now/)).toBeInTheDocument();

      await user.click(d.getByRole('button', { name: 'Save current edit as preset…' }));
      await user.type(d.getByRole('textbox', { name: 'Preset name' }), 'Another');
      await user.click(d.getByRole('button', { name: 'Save preset' }));
      await waitFor(() =>
        expect(lastPresets().map((p) => p.name)).toEqual(['Contrasty', 'Another']),
      );
      expect(ipc.recipeMergeFields).not.toHaveBeenCalled();
    });

    it('says so when the preset could not be applied, and changes nothing', async () => {
      state.developPresets = [preset('Contrasty')];
      ipc.recipeMergeFields.mockRejectedValueOnce(
        new ipc.IpcError({ code: 'Io', message: 'Broken.' }),
      );
      const { user } = await openFrame();
      await user.click(button('Presets…'));
      await user.click(
        await inDialog().findByRole('button', { name: 'Apply the preset Contrasty' }),
      );
      expect(await inDialog().findByRole('alert')).toHaveTextContent(
        'The preset could not be applied: Broken.',
      );
      await user.click(inDialog().getByRole('button', { name: 'Close' }));
      expect(value('Contrast')).toBe(0);
      expect(button('Undo')).toBeDisabled();
    });
  });

  it('has no accessibility violations', async () => {
    state.developPresets = [preset('Warm', ['Color', 'Tone'])];
    const user = await openPresets();
    const d = inDialog();
    await d.findByText('Warm');
    await expectNoA11yViolations(dialog());
    await user.click(d.getByRole('button', { name: 'Save current edit as preset…' }));
    await user.type(d.getByRole('textbox', { name: 'Preset name' }), 'warm');
    await expectNoA11yViolations(dialog());
  });
});
