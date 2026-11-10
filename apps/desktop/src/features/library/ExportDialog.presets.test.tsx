import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ExportPreset } from '../../shared/ipc';
import { expectNoA11yViolations, mockLayout } from '../../test-support';
import {
  DEFAULT_EXPORT_PRESETS,
  libraryDetails,
  listeners,
  makeFrames,
  makeRoll,
  resetFake,
  state,
} from './fake-ipc';
import { resetLibraryStore, useLibraryStore } from './library-store';
import { LibraryView } from './LibraryView';
import { resetThumbnails } from './thumbnails';

const [ARCHIVE, SHARE] = DEFAULT_EXPORT_PRESETS as [ExportPreset, ExportPreset];

vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);

const dialog = () => screen.getByRole('dialog', { name: 'Export' });
const presetSelect = () => within(dialog()).getByRole('combobox', { name: 'Preset' });
const colourSpace = () => within(dialog()).getByRole('combobox', { name: /Colour space/ });
const button = (name: string) => within(dialog()).getByRole('button', { name });

/** Opens the library, selects two frames and opens the export dialog, with its settings loaded. */
async function openDialog() {
  const user = userEvent.setup();
  state.details = libraryDetails();
  state.rolls = [makeRoll({ id: 'summer', name: 'Summer' })];
  state.frames = makeFrames('summer', 3);
  render(<LibraryView />);
  await screen.findAllByRole('gridcell');
  act(() => useLibraryStore.getState().setSelected(['summer-f00001', 'summer-f00002']));
  await user.click(screen.getByRole('button', { name: 'Export…' }));
  await screen.findByRole('dialog', { name: 'Export' });
  await waitFor(() =>
    expect(within(presetSelect()).getAllByRole('option').length).toBeGreaterThan(1),
  );
  return user;
}

async function chooseFolder(user: ReturnType<typeof userEvent.setup>) {
  await user.click(button('Choose folder…'));
  await within(dialog()).findByText('/films/Library');
}

describe('ExportDialog presets', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    restoreLayout = mockLayout(900, 600);
  });
  afterEach(() => restoreLayout());

  it('offers No preset and the presets, starting on No preset', async () => {
    await openDialog();
    expect(presetSelect()).toHaveValue('');
    const names = within(presetSelect())
      .getAllByRole('option')
      .map((o) => o.textContent);
    expect(names).toEqual(['No preset', 'Archive (TIFF 16-bit)', 'Share (JPEG sRGB)']);
    expect(button('Update')).toBeDisabled();
    expect(button('Delete')).toBeDisabled();
  });

  it('fills all the options when a preset is chosen, and keeps the folder of a preset without one', async () => {
    const user = await openDialog();
    await chooseFolder(user);
    await user.selectOptions(presetSelect(), 'Archive (TIFF 16-bit)');
    expect(within(dialog()).getByRole('radio', { name: /TIFF, 16-bit/ })).toBeChecked();
    expect(colourSpace()).toHaveValue('prophoto');
    expect(within(dialog()).getByText('/films/Library')).toBeInTheDocument();

    await user.selectOptions(presetSelect(), 'Share (JPEG sRGB)');
    expect(within(dialog()).getByRole('radio', { name: /JPEG/ })).toBeChecked();
    expect(colourSpace()).toHaveValue('srgb');
    expect(within(dialog()).getByRole('slider', { name: 'JPEG quality' })).toHaveAttribute(
      'aria-valuenow',
      '90',
    );
    expect(within(dialog()).getByText('/films/Library')).toBeInTheDocument();
  });

  it('takes the folder of a preset that has one', async () => {
    state.exportPresets = [
      { name: 'To disk', options: { ...ARCHIVE.options, dest_dir: '/disk/out' } },
    ];
    const user = await openDialog();
    await chooseFolder(user);
    await user.selectOptions(presetSelect(), 'To disk');
    expect(within(dialog()).getByText('/disk/out')).toBeInTheDocument();
    expect(within(dialog()).queryByText('/films/Library')).not.toBeInTheDocument();
  });

  it('shows the preset as modified when an option differs, and not when only the folder does', async () => {
    const user = await openDialog();
    await user.selectOptions(presetSelect(), 'Archive (TIFF 16-bit)');
    expect(button('Update')).toBeDisabled();
    await chooseFolder(user);
    expect(
      within(presetSelect()).getByRole('option', { name: 'Archive (TIFF 16-bit)' }),
    ).toBeInTheDocument();
    expect(button('Update')).toBeDisabled();

    await user.selectOptions(colourSpace(), 'adobe_rgb');
    expect(
      within(presetSelect()).getByRole('option', { name: 'Archive (TIFF 16-bit) (modified)' }),
    ).toBeInTheDocument();
    expect(button('Update')).toBeEnabled();

    await user.selectOptions(colourSpace(), 'prophoto');
    expect(button('Update')).toBeDisabled();
  });

  describe('Save as preset', () => {
    it('asks for a name inline, saves the options shown, and chooses the new preset', async () => {
      const user = await openDialog();
      await chooseFolder(user);
      await user.click(within(dialog()).getByRole('radio', { name: /JPEG/ }));
      await user.selectOptions(colourSpace(), 'adobe_rgb');
      await user.click(button('Save as preset…'));
      await user.type(within(dialog()).getByRole('textbox', { name: 'Preset name' }), '  Print  ');
      await user.click(button('Save preset'));

      await waitFor(() => expect(presetSelect()).toHaveValue('Print'));
      expect(state.exportPresets).toHaveLength(3);
      expect(state.exportPresets[2]).toEqual({
        name: 'Print',
        options: {
          format: 'jpeg',
          space: 'adobe_rgb',
          jpeg_quality: 92,
          dest_dir: '/films/Library',
          template: '{roll}_{index:03}',
        },
      });
      expect(within(dialog()).queryByRole('textbox', { name: 'Preset name' })).toBeNull();
      expect(button('Update')).toBeDisabled();
    });

    it('refuses an empty name and a name in use, with an alert tied to the field', async () => {
      const user = await openDialog();
      await user.click(button('Save as preset…'));
      const name = within(dialog()).getByRole('textbox', { name: 'Preset name' });
      await user.click(button('Save preset'));
      const alert = await within(dialog()).findByRole('alert');
      expect(alert).toHaveTextContent('Type a name for the preset.');
      expect(name).toHaveAttribute('aria-invalid', 'true');
      expect(name).toHaveAccessibleDescription('Type a name for the preset.');

      await user.type(name, 'share (jpeg srgb)');
      expect(within(dialog()).queryByRole('alert')).not.toBeInTheDocument();
      await user.click(button('Save preset'));
      expect(await within(dialog()).findByRole('alert')).toHaveTextContent(
        'A preset with that name already exists.',
      );
      expect(state.exportPresets).toHaveLength(2);
      expect(state.settingsPatches).toEqual([]);

      await user.click(button('Cancel'));
      expect(within(dialog()).queryByRole('textbox', { name: 'Preset name' })).toBeNull();
    });
  });

  it('Update overwrites the chosen preset with the options shown, folder included', async () => {
    const user = await openDialog();
    await user.selectOptions(presetSelect(), 'Share (JPEG sRGB)');
    await chooseFolder(user);
    await user.selectOptions(colourSpace(), 'adobe_rgb');
    await user.click(button('Update'));

    await waitFor(() => expect(button('Update')).toBeDisabled());
    expect(state.exportPresets.map((p) => p.name)).toEqual([
      'Archive (TIFF 16-bit)',
      'Share (JPEG sRGB)',
    ]);
    expect(state.exportPresets.at(1)?.options).toEqual({
      format: 'jpeg',
      space: 'adobe_rgb',
      jpeg_quality: 90,
      dest_dir: '/films/Library',
      template: '{roll}_{index:03}',
    });
    expect(state.exportPresets[0]).toEqual(ARCHIVE);
    expect(presetSelect()).toHaveValue('Share (JPEG sRGB)');
  });

  it('Delete asks for confirmation, and can be called off', async () => {
    const user = await openDialog();
    await user.selectOptions(presetSelect(), 'Share (JPEG sRGB)');
    await user.click(button('Delete'));
    const group = within(dialog()).getByRole('group', { name: 'Delete preset' });
    expect(group).toHaveTextContent('Delete the preset “Share (JPEG sRGB)”?');
    expect(state.exportPresets).toHaveLength(2);

    await user.click(within(group).getByRole('button', { name: 'Keep it' }));
    expect(within(dialog()).queryByRole('group', { name: 'Delete preset' })).toBeNull();
    expect(state.exportPresets).toHaveLength(2);

    await user.click(button('Delete'));
    await user.click(within(dialog()).getByRole('button', { name: 'Delete preset' }));
    await waitFor(() => expect(presetSelect()).toHaveValue(''));
    expect(state.exportPresets.map((p) => p.name)).toEqual(['Archive (TIFF 16-bit)']);
    // The options stay as they were.
    expect(within(dialog()).getByRole('radio', { name: /JPEG/ })).toBeChecked();
  });

  it('starts from the last used options and preset when it opens again', async () => {
    const user = await openDialog();
    await chooseFolder(user);
    await user.selectOptions(presetSelect(), 'Share (JPEG sRGB)');
    await user.selectOptions(colourSpace(), 'adobe_rgb');
    await user.click(button('Export'));
    await within(dialog()).findByRole('progressbar', { name: 'Export 2 frames' });

    expect(state.lastExport).toEqual({
      options: {
        format: 'jpeg',
        space: 'adobe_rgb',
        jpeg_quality: 90,
        dest_dir: '/films/Library',
        template: '{roll}_{index:03}',
      },
      preset: 'Share (JPEG sRGB)',
    });

    act(() =>
      listeners.done.forEach((cb) =>
        cb({
          id: 'job-export-1',
          result: { kind: 'Exported', frame_ms: [], files: [], failed: [] } as never,
        }),
      ),
    );
    await user.click(await within(dialog()).findByRole('button', { name: 'Close' }));
    act(() => useLibraryStore.getState().dismissJob('job-export-1'));
    await user.click(screen.getByRole('button', { name: 'Export…' }));
    await screen.findByRole('dialog', { name: 'Export' });

    await waitFor(() => expect(presetSelect()).toHaveValue('Share (JPEG sRGB)'));
    expect(colourSpace()).toHaveValue('adobe_rgb');
    expect(within(dialog()).getByText('/films/Library')).toBeInTheDocument();
    expect(
      within(presetSelect()).getByRole('option', { name: 'Share (JPEG sRGB) (modified)' }),
    ).toBeInTheDocument();
  });

  it('starts on No preset when the last preset no longer exists', async () => {
    state.exportPresets = [];
    state.lastExport = {
      options: { ...SHARE.options, dest_dir: '/out' },
      preset: 'Gone',
    };
    const user = userEvent.setup();
    state.details = libraryDetails();
    state.rolls = [makeRoll({ id: 'summer', name: 'Summer' })];
    state.frames = makeFrames('summer', 3);
    render(<LibraryView />);
    await screen.findAllByRole('gridcell');
    act(() => useLibraryStore.getState().setSelected(['summer-f00001']));
    await user.click(screen.getByRole('button', { name: 'Export…' }));
    await screen.findByRole('dialog', { name: 'Export' });
    await within(dialog()).findByText('/out');
    expect(presetSelect()).toHaveValue('');
    expect(within(dialog()).getByRole('radio', { name: /JPEG/ })).toBeChecked();
  });

  it('shows why presets could not be saved, and leaves the list as it was', async () => {
    const user = await openDialog();
    state.settingsRefused = 'The disk is full.';
    await user.click(button('Save as preset…'));
    await user.type(within(dialog()).getByRole('textbox', { name: 'Preset name' }), 'Print');
    await user.click(button('Save preset'));
    expect(await within(dialog()).findByRole('alert')).toHaveTextContent(
      'The presets could not be saved: The disk is full.',
    );
    expect(within(presetSelect()).queryByRole('option', { name: 'Print' })).toBeNull();
    // The name stays to try again.
    expect(within(dialog()).getByRole('textbox', { name: 'Preset name' })).toHaveValue('Print');
  });

  it('shows why presets could not be loaded, and still exports', async () => {
    state.settingsUnreadable = 'The settings file is damaged.';
    const user = userEvent.setup();
    state.details = libraryDetails();
    state.rolls = [makeRoll({ id: 'summer', name: 'Summer' })];
    state.frames = makeFrames('summer', 3);
    render(<LibraryView />);
    await screen.findAllByRole('gridcell');
    act(() => useLibraryStore.getState().setSelected(['summer-f00001']));
    await user.click(screen.getByRole('button', { name: 'Export…' }));
    await screen.findByRole('dialog', { name: 'Export' });
    expect(await within(dialog()).findByRole('alert')).toHaveTextContent(
      'The presets could not be loaded: The settings file is damaged.',
    );
    expect(button('Choose folder…')).toBeEnabled();
  });

  it('has no accessibility violations: the preset controls, naming and deleting', async () => {
    const user = await openDialog();
    await expectNoA11yViolations(document.body);
    await user.selectOptions(presetSelect(), 'Archive (TIFF 16-bit)');
    await user.selectOptions(colourSpace(), 'adobe_rgb');
    await expectNoA11yViolations(document.body);
    await user.click(button('Save as preset…'));
    await user.click(button('Save preset'));
    await within(dialog()).findByRole('alert');
    await expectNoA11yViolations(document.body);
    await user.click(button('Cancel'));
    await user.click(button('Delete'));
    await expectNoA11yViolations(document.body);
  });
});
