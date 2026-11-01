import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import { expectNoA11yViolations, mockLayout } from '../../test-support';
import {
  PIXELS,
  ipc,
  libraryDetails,
  makeFrames,
  makeRoll,
  resetFake,
  state,
} from '../library/fake-ipc';
import { resetLibraryStore, useLibraryStore } from '../library/library-store';
import { resetThumbnails } from '../library/thumbnails';
import { DevelopView } from './DevelopView';
import { baseOf, levelsOf } from './recipe-parts';
import { RELEASE_EDGE, SAVE_DELAY_MS, WHOLE_SCAN_GEOMETRY } from './use-develop';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

/** A library with the roll "Summer" (5 frames) open and chosen, as the Library leaves it. */
function openLibrary(details = {}) {
  state.details = libraryDetails(details);
  state.rolls = [makeRoll({ id: 'summer', name: 'Summer' })];
  state.frames = makeFrames('summer', 5);
  useLibraryStore.setState({
    phase: 'ready',
    library: state.details,
    rolls: state.rolls,
    rollId: 'summer',
  });
}

/** The filmstrip (the panels have lists of options of their own). */
const strip = () => screen.getByRole('listbox', { name: 'Frames of the current Library view' });

const open = (n: number) =>
  useUiStore.setState({
    view: 'develop',
    developFrame: { id: `summer-f${String(n).padStart(5, '0')}`, index: n - 1 },
  });

/**
 * The picture, found with a plain selector: role-and-name queries are slow in a DOM with a
 * histogram in it and can run out of time when the whole suite is busy.
 */
const canvasNamed = (name: string | RegExp = /^Frame \d$/) =>
  waitFor(() => {
    const element = document.querySelector<HTMLCanvasElement>('canvas[role="img"]');
    const label = element?.getAttribute('aria-label') ?? '';
    if (!element || !(typeof name === 'string' ? label === name : name.test(label))) {
      throw new Error(`no picture named ${String(name)}, found "${label}"`);
    }
    return element;
  });
const canvas = () => canvasNamed();
const frame = () => useUiStore.getState().developFrame;

describe('DevelopView', () => {
  let restoreLayout: () => void;
  const saved = [...(PIXELS.saved ?? [])] as [number, number, number];

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    useUiStore.setState({ view: 'develop', developFrame: null });
    // A stage of 900 x 600 for the picture to fit in; no canvas to draw on in jsdom.
    restoreLayout = mockLayout(900, 600);
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => null);
  });
  afterEach(() => {
    restoreLayout();
    PIXELS.saved = saved;
    vi.restoreAllMocks();
  });

  describe('without a frame', () => {
    it('says how to open one, and goes back to the Library', async () => {
      const user = userEvent.setup();
      render(<DevelopView />);
      expect(screen.getByText(/double-click it, or select it and press Enter/)).toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Library' }));
      expect(useUiStore.getState().view).toBe('library');
      expect(ipc.developOpen).not.toHaveBeenCalled();
    });
  });

  describe('an open frame', () => {
    it('shows the frame on a canvas with its histogram and what is clipped', async () => {
      openLibrary();
      open(2);
      render(<DevelopView />);
      expect(screen.getByText('Opening the frame…')).toBeInTheDocument();
      expect(await canvas()).toHaveAccessibleName('Frame 2');
      expect(ipc.developOpen).toHaveBeenCalledWith('summer-f00002');
      expect(ipc.developRender).toHaveBeenCalledWith(
        'summer-f00002',
        expect.anything(),
        2048,
        expect.any(Number),
      );
      expect(
        screen.getByRole('img', { name: 'Histogram of the picture shown' }),
      ).toBeInTheDocument();
      expect(screen.getByText('Black clipped: 0%. White clipped: 0%.')).toBeInTheDocument();
    });

    it('says how much of the picture is lost to black and white', async () => {
      PIXELS.saved = [255, 255, 255];
      openLibrary();
      open(1);
      render(<DevelopView />);
      expect(
        await screen.findByText('Black clipped: 0%. White clipped: 100%.'),
      ).toBeInTheDocument();
    });

    it('says why a frame could not be opened, and still lets you leave', async () => {
      state.noScan.add('summer-f00003');
      openLibrary();
      open(3);
      render(<DevelopView />);
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The frame could not be opened: This frame has no scan file.',
      );
      expect(screen.queryByRole('img', { name: /^Frame/ })).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Library' })).toBeEnabled();
    });

    it('reports a picture that could not be updated and keeps the last one', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(<DevelopView />);
      await canvas();
      await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalled());
      // The next picture asked for (the "before" one) fails.
      ipc.developRender.mockRejectedValueOnce(new Error('The recipe could not be rendered.'));
      await user.click(screen.getByRole('button', { name: 'Before' }));
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'The picture could not be updated: The recipe could not be rendered.',
      );
      expect(screen.getByRole('img', { name: 'Frame 1' })).toBeInTheDocument();
    });
  });

  describe('before and after', () => {
    it('switches with the button and with the backslash key, and says which is shown', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(<DevelopView />);
      await canvas();
      await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalled());
      const toggle = screen.getByRole('button', { name: 'Before' });
      expect(toggle).toHaveAttribute('aria-pressed', 'false');
      expect(screen.queryByText('Before', { selector: 'span' })).not.toBeInTheDocument();

      await user.click(toggle);
      expect(await screen.findByText(/Showing the automatic conversion/)).toBeInTheDocument();
      expect(toggle).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByText('Before', { selector: 'span' })).toBeInTheDocument();
      // The histogram follows what is shown.
      const calls = ipc.developRender.mock.calls;
      expect(calls[calls.length - 1]?.[1]).toMatchObject({ tag: 'auto' });

      await user.keyboard('\\');
      expect(toggle).toHaveAttribute('aria-pressed', 'false');
      expect(screen.queryByText(/Showing the automatic conversion/)).not.toBeInTheDocument();
      await user.keyboard('\\');
      expect(toggle).toHaveAttribute('aria-pressed', 'true');
    });

    it('is disabled until the frame is open, and says when the conversion cannot be made', async () => {
      const user = userEvent.setup();
      state.noAnalysis.add('summer-f00001');
      openLibrary();
      open(1);
      render(<DevelopView />);
      expect(screen.getByRole('button', { name: 'Before' })).toBeDisabled();
      await canvas();
      await user.click(screen.getByRole('button', { name: 'Before' }));
      expect(await screen.findByText(/there is no “before” picture/)).toBeInTheDocument();
    });
  });

  describe('the filmstrip', () => {
    it('lists the frames of the Library view with the open one marked', async () => {
      openLibrary();
      open(2);
      render(<DevelopView />);
      await canvas();
      const strip = screen.getByRole('listbox', { name: 'Frames of the current Library view' });
      const options = await within(strip).findAllByRole('option');
      expect(options).toHaveLength(5);
      expect(options.map((o) => o.getAttribute('aria-selected'))).toEqual([
        'false',
        'true',
        'false',
        'false',
        'false',
      ]);
      expect(options[1]).toHaveAccessibleName('Frame 2');
      expect(strip).toHaveAttribute('aria-activedescendant', options[1]?.id);
    });

    it('lists what a search found, as the Library did', async () => {
      openLibrary();
      useLibraryStore.setState({
        filters: { text: 'summer', tagIds: [], film: '', camera: '', iso: '', from: '', to: '' },
      });
      open(1);
      render(<DevelopView />);
      await canvas();
      await waitFor(() => expect(ipc.search).toHaveBeenCalled());
      expect(await within(strip()).findAllByRole('option')).toHaveLength(5);
    });

    it('opens another frame when it is clicked', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(<DevelopView />);
      await canvas();
      const options = await within(strip()).findAllByRole('option');
      await user.click(options[3] as HTMLElement);
      expect(frame()).toEqual({ id: 'summer-f00004', index: 3 });
      expect(await canvasNamed('Frame 4')).toBeInTheDocument();
      expect(ipc.developOpen).toHaveBeenLastCalledWith('summer-f00004');
    });

    it('moves to the next and previous frame with ] and [, and stops at the ends', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(<DevelopView />);
      await canvas();
      await screen.findAllByRole('option');

      // ("[[" is how user-event types a single "[".)
      await user.keyboard('[[');
      expect(frame()).toEqual({ id: 'summer-f00001', index: 0 });
      await user.keyboard(']');
      await waitFor(() => expect(frame()).toEqual({ id: 'summer-f00002', index: 1 }));
      await user.keyboard(']]');
      await waitFor(() => expect(frame()).toEqual({ id: 'summer-f00004', index: 3 }));
      await user.keyboard('[[');
      await waitFor(() => expect(frame()).toEqual({ id: 'summer-f00003', index: 2 }));
      expect(await canvasNamed('Frame 3')).toBeInTheDocument();

      await user.keyboard(']]]');
      await waitFor(() => expect(frame()).toEqual({ id: 'summer-f00005', index: 4 }));
      await user.keyboard(']');
      expect(frame()).toEqual({ id: 'summer-f00005', index: 4 });
    });

    it('moves with the arrow keys while the strip has focus', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(2);
      render(<DevelopView />);
      await canvas();
      screen.getByRole('listbox').focus();
      await user.keyboard('{ArrowRight}');
      await waitFor(() => expect(frame()?.index).toBe(2));
      await user.keyboard('{ArrowLeft}{ArrowLeft}');
      await waitFor(() => expect(frame()?.index).toBe(0));
    });

    it('starts the next frame fitted again, and with "before" off', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(<DevelopView />);
      await canvas();
      await user.keyboard('1');
      expect(screen.getByRole('button', { name: 'Fit' })).toBeEnabled();
      await user.keyboard(']');
      await canvasNamed('Frame 2');
      await waitFor(() => expect(screen.getByRole('button', { name: 'Fit' })).toBeDisabled());
    });
  });

  describe('leaving', () => {
    it('goes back to the Library with Escape and with the button', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(<DevelopView />);
      await canvas();
      await user.keyboard('{Escape}');
      expect(useUiStore.getState().view).toBe('library');

      useUiStore.setState({ view: 'develop' });
      await user.click(screen.getByRole('button', { name: 'Library' }));
      expect(useUiStore.getState().view).toBe('library');
    });
  });

  describe('pan and zoom', () => {
    const zoomText = () => screen.getByLabelText('Zoom level').textContent;

    it('starts fitted, zooms with the buttons and the keys, and shows the zoom', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(<DevelopView />);
      await canvas();
      // 900 x 600 for a 3:2 picture: the height limits, about 44%.
      expect(zoomText()).toMatch(/^4[34]/);
      expect(screen.getByRole('button', { name: 'Fit' })).toBeDisabled();

      await user.click(screen.getByRole('button', { name: 'Zoom in' }));
      const zoomedIn = Number.parseFloat(zoomText() ?? '0');
      expect(zoomedIn).toBeGreaterThan(50);
      expect(screen.getByRole('button', { name: 'Fit' })).toBeEnabled();

      await user.click(screen.getByRole('button', { name: '100%' }));
      expect(zoomText()).toBe('100%');
      await user.keyboard('+');
      expect(zoomText()).toBe('125%');
      await user.keyboard('-');
      expect(zoomText()).toBe('100%');
      await user.keyboard('0');
      expect(zoomText()).toMatch(/^4[34]/);
      await user.keyboard('1');
      expect(zoomText()).toBe('100%');
      await user.click(screen.getByRole('button', { name: 'Fit' }));
      expect(zoomText()).toMatch(/^4[34]/);
    });

    it('never goes below the fit or past 400%', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(<DevelopView />);
      await canvas();
      await user.click(screen.getByRole('button', { name: 'Zoom out' }));
      expect(zoomText()).toMatch(/^4[34]/);
      for (let i = 0; i < 12; i += 1)
        await user.click(screen.getByRole('button', { name: 'Zoom in' }));
      expect(zoomText()).toBe('400%');
    });

    it('zooms with the wheel and pans by dragging, once zoomed', async () => {
      openLibrary();
      open(1);
      const { container } = render(<DevelopView />);
      const picture = await canvas();
      const stage = container.querySelector('[aria-busy]') as HTMLElement;
      const move = () => picture.style.transform;

      // Fitted: nothing to drag.
      fireEvent.mouseDown(stage, { button: 0, clientX: 100, clientY: 100 });
      fireEvent.mouseMove(window, { clientX: 40, clientY: 60 });
      fireEvent.mouseUp(window);
      expect(move()).toBe('translate(-50%, -50%) translate(0px, 0px)');

      fireEvent.wheel(stage, { deltaY: -100, clientX: 450, clientY: 300 });
      fireEvent.wheel(stage, { deltaY: -100, clientX: 450, clientY: 300 });
      await waitFor(() => expect(Number.parseFloat(zoomText() ?? '0')).toBeGreaterThan(60));
      fireEvent.wheel(stage, { deltaY: 100, clientX: 450, clientY: 300 });
      fireEvent.wheel(stage, { deltaY: 100, clientX: 450, clientY: 300 });
      fireEvent.wheel(stage, { deltaY: 100, clientX: 450, clientY: 300 });
      await waitFor(() => expect(screen.getByRole('button', { name: 'Fit' })).toBeDisabled());

      // At 100% the picture is larger than the stage and can be dragged.
      fireEvent.keyDown(window, { key: '1' });
      expect(zoomText()).toBe('100%');
      fireEvent.mouseDown(stage, { button: 0, clientX: 100, clientY: 100 });
      fireEvent.mouseMove(window, { clientX: 60, clientY: 80 });
      fireEvent.mouseUp(window);
      expect(move()).toBe('translate(-50%, -50%) translate(-40px, -20px)');
      // A right-button press does not pan; after the button is released, moving does nothing.
      fireEvent.mouseDown(stage, { button: 2, clientX: 0, clientY: 0 });
      fireEvent.mouseMove(window, { clientX: 500, clientY: 500 });
      fireEvent.mouseMove(window, { clientX: 0, clientY: 0 });
      expect(move()).toBe('translate(-50%, -50%) translate(-40px, -20px)');
    });

    it('leaves the keys alone while typing in a field', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(
        <>
          <input aria-label="Note" />
          <DevelopView />
        </>,
      );
      await canvas();
      await user.click(screen.getByRole('textbox', { name: 'Note' }));
      await user.keyboard('1]\\');
      expect(screen.getByRole('textbox', { name: 'Note' })).toHaveValue('1]\\');
      expect(zoomText()).toMatch(/^4[34]/);
      expect(frame()?.index).toBe(0);
      expect(screen.getByRole('button', { name: 'Before' })).toHaveAttribute(
        'aria-pressed',
        'false',
      );
    });
  });

  describe('the colour keys', () => {
    const colourOf = (recipe: unknown) => (recipe as { color: Record<string, number> }).color;
    const lastRender = () => ipc.developRender.mock.calls.at(-1)?.[1];
    const savedRecipe = () => state.saved[state.saved.length - 1]?.recipe;

    async function openFrame() {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(<DevelopView />);
      await canvas();
      // The settings, with the keys the user changed, have been read.
      await waitFor(() => expect(ipc.settingsGet).toHaveBeenCalled());
      return user;
    }

    it.each([
      ['q', 'red_cyan', 10],
      ['a', 'red_cyan', -10],
      ['w', 'green_magenta', 10],
      ['s', 'green_magenta', -10],
      ['e', 'blue_yellow', 10],
      ['d', 'blue_yellow', -10],
    ])('%s moves %s by %i steps, previews it and saves it', async (key, axis, amount) => {
      const user = await openFrame();
      ipc.developRender.mockClear();
      await user.keyboard(key);
      await waitFor(() => expect(ipc.developRender).toHaveBeenCalled());
      expect(colourOf(lastRender())[axis]).toBe(amount);
      await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(1), {
        timeout: SAVE_DELAY_MS * 4,
      });
      expect(colourOf(savedRecipe())[axis]).toBe(amount);
    });

    it('moves the slider you can see, and builds on the press before', async () => {
      const user = await openFrame();
      await user.keyboard('qq');
      await waitFor(() =>
        expect(screen.getByRole('slider', { name: 'Cyan / red' })).toHaveAttribute(
          'aria-valuenow',
          '20',
        ),
      );
      await user.keyboard('a');
      await waitFor(() =>
        expect(screen.getByRole('slider', { name: 'Cyan / red' })).toHaveAttribute(
          'aria-valuenow',
          '10',
        ),
      );
    });

    it('moves it by one step with Shift', async () => {
      const user = await openFrame();
      await user.keyboard('{Shift>}q{/Shift}');
      await waitFor(() => expect(colourOf(lastRender()).red_cyan).toBe(1));
      await user.keyboard('{Shift>}D{/Shift}');
      await waitFor(() => expect(colourOf(lastRender()).blue_yellow).toBe(-1));
    });

    it('stops at the end of the slider', async () => {
      const user = await openFrame();
      for (let i = 0; i < 12; i += 1) await user.keyboard('q');
      await waitFor(() => expect(colourOf(lastRender()).red_cyan).toBe(100));
      ipc.developRender.mockClear();
      await user.keyboard('q');
      expect(ipc.developRender).not.toHaveBeenCalled();
      await user.keyboard('a');
      await waitFor(() => expect(colourOf(lastRender()).red_cyan).toBe(90));
    });

    it('draws a lighter preview while a key is held, and the sharp one when it is let go', async () => {
      await openFrame();
      ipc.developRender.mockClear();
      fireEvent.keyDown(window, { key: 'q', repeat: false });
      fireEvent.keyDown(window, { key: 'q', repeat: true });
      fireEvent.keyDown(window, { key: 'q', repeat: true });
      await waitFor(() => expect(colourOf(lastRender()).red_cyan).toBe(30));
      ipc.developRender.mockClear();
      fireEvent.keyUp(window, { key: 'q' });
      await waitFor(() => expect(ipc.developRender).toHaveBeenCalledTimes(1));
      expect(ipc.developRender.mock.calls[0]?.[2]).toBe(RELEASE_EDGE);
      expect(colourOf(ipc.developRender.mock.calls[0]?.[1]).red_cyan).toBe(30);
      // Letting go of a key that was not held does nothing more.
      ipc.developRender.mockClear();
      fireEvent.keyUp(window, { key: 'q' });
      expect(ipc.developRender).not.toHaveBeenCalled();
    });

    it('uses the keys the user chose in place of the defaults', async () => {
      state.keys = { 'colour.red': 'r', 'develop.fit': 'f' };
      const user = await openFrame();
      await waitFor(() => expect(ipc.settingsGet).toHaveBeenCalled());
      await new Promise((resolve) => setTimeout(resolve, 20));
      await user.keyboard('q');
      expect(ipc.recipeSave).not.toHaveBeenCalled();
      await user.keyboard('r');
      await waitFor(() => expect(colourOf(lastRender()).red_cyan).toBe(10));
    });

    it('does nothing with Ctrl, Alt or Command, or in a field, or in a dialog', async () => {
      const user = await openFrame();
      ipc.developRender.mockClear();
      await user.keyboard('{Control>}q{/Control}{Alt>}q{/Alt}{Meta>}q{/Meta}');
      const field = document.createElement('input');
      document.body.append(field);
      field.focus();
      await user.keyboard('qaw');
      field.remove();
      await user.click(screen.getByRole('button', { name: 'Export…' }));
      await screen.findByRole('dialog', { name: 'Export' });
      await user.keyboard('q');
      expect(ipc.developRender).not.toHaveBeenCalled();
      expect(ipc.recipeSave).not.toHaveBeenCalled();
    });

    it('lets Escape close a dialog without also leaving the view', async () => {
      const user = await openFrame();
      await user.click(screen.getByRole('button', { name: 'Export…' }));
      await screen.findByRole('dialog', { name: 'Export' });
      await user.keyboard('{Escape}');
      await waitFor(() =>
        expect(screen.queryByRole('dialog', { name: 'Export' })).not.toBeInTheDocument(),
      );
      expect(useUiStore.getState().view).toBe('develop');
      await user.keyboard('{Escape}');
      expect(useUiStore.getState().view).toBe('library');
    });

    it('does nothing while something is being picked, or in a library that cannot be changed', async () => {
      const user = await openFrame();
      await user.click(screen.getByRole('button', { name: 'Pick film base' }));
      await screen.findByText(/Click the clear film edge/);
      ipc.developRender.mockClear();
      await user.keyboard('q');
      expect(ipc.developRender).not.toHaveBeenCalled();
    });

    it('does nothing in a library that cannot be changed', async () => {
      const user = userEvent.setup();
      openLibrary({ read_only: true });
      open(1);
      render(<DevelopView />);
      await canvas();
      ipc.developRender.mockClear();
      await user.keyboard('q');
      expect(ipc.developRender).not.toHaveBeenCalled();
      expect(ipc.recipeSave).not.toHaveBeenCalled();
    });

    it('still has the other keys: before and after, and fit', async () => {
      const user = await openFrame();
      await user.keyboard('\\');
      expect(screen.getByRole('button', { name: 'Before' })).toHaveAttribute(
        'aria-pressed',
        'true',
      );
    });
  });

  describe('applying settings to other frames', () => {
    const applyButton = () => screen.getByRole('button', { name: 'Apply settings…' });

    async function openApply() {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(<DevelopView />);
      await canvas();
      await user.click(applyButton());
      const dialog = within(await screen.findByRole('dialog', { name: 'Apply settings' }));
      await dialog.findByRole('radio', { name: /All other frames of Summer/ });
      return { user, dialog };
    }

    it('copies from the open frame to the rest of its roll when nothing is selected', async () => {
      const { user, dialog } = await openApply();
      expect(dialog.getByText('Copy from Frame 1 of Summer.')).toBeInTheDocument();
      expect(dialog.getByRole('radio', { name: 'The other selected frames (0)' })).toBeDisabled();
      expect(dialog.getByRole('radio', { name: 'All other frames of Summer (4)' })).toBeChecked();

      await user.click(dialog.getByRole('button', { name: 'Apply colour and tone to 4 frames' }));
      await waitFor(() => expect(state.applies).toHaveLength(1));
      expect(state.applies[0]).toEqual({
        source: 'summer-f00001',
        frameIds: ['summer-f00002', 'summer-f00003', 'summer-f00004', 'summer-f00005'],
        fields: ['Color', 'Tone'],
      });
      expect(await dialog.findByText('Applied to 4 frames.')).toBeInTheDocument();
    });

    it('offers the frames selected in the Library, leaving out the open one', async () => {
      useLibraryStore.getState().setSelected(['summer-f00001', 'summer-f00003']);
      const { user, dialog } = await openApply();
      expect(dialog.getByRole('radio', { name: 'The other selected frame (1)' })).toBeChecked();
      await user.click(dialog.getByRole('button', { name: 'Apply colour and tone to 1 frame' }));
      await waitFor(() => expect(state.applies).toHaveLength(1));
      expect(state.applies[0]?.frameIds).toEqual(['summer-f00003']);
    });

    it('is out of reach in a library that cannot be changed', async () => {
      openLibrary({ read_only: true });
      open(1);
      render(<DevelopView />);
      await canvas();
      expect(applyButton()).toBeDisabled();
    });
  });

  describe('exporting', () => {
    it('exports the open frame from the toolbar, and follows the job in the dialog', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(2);
      render(<DevelopView />);
      await canvas();
      await user.click(screen.getByRole('button', { name: 'Export…' }));
      const dialog = within(await screen.findByRole('dialog', { name: 'Export' }));
      expect(
        dialog.getByText('Write 1 frame at full size, with the colour profile embedded.'),
      ).toBeInTheDocument();

      await user.click(dialog.getByRole('button', { name: 'Choose folder…' }));
      await dialog.findByText('/films/Library');
      await user.click(dialog.getByRole('button', { name: 'Export' }));
      await waitFor(() => expect(state.exports).toHaveLength(1));
      expect(state.exports[0]?.frameIds).toEqual(['summer-f00002']);
      expect(
        await dialog.findByRole('progressbar', { name: 'Export 1 frame' }),
      ).toBeInTheDocument();

      await user.click(dialog.getByRole('button', { name: 'Close' }));
      expect(screen.queryByRole('dialog', { name: 'Export' })).not.toBeInTheDocument();
    });
  });

  describe('the Base and Invert panel', () => {
    const rect = (left: number, top: number, width: number, height: number) =>
      ({ left, top, width, height, right: left + width, bottom: top + height }) as DOMRect;

    beforeEach(() => {
      // jsdom does no layout: the picture is 400 x 300 at (100, 50) on a stage of 900 x 600.
      vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
        this: HTMLElement,
      ) {
        if (this.tagName === 'CANVAS' || this.className.includes('cropLayer')) {
          return rect(100, 50, 400, 300);
        }
        if (this.hasAttribute('aria-busy')) return rect(0, 0, 900, 600);
        return rect(0, 0, 0, 0);
      });
    });

    const stage = () => document.querySelector('[aria-busy]') as HTMLElement;
    const press = (x: number, y: number) =>
      fireEvent.mouseDown(stage(), { button: 0, clientX: x, clientY: y });
    const release = (x: number, y: number) => fireEvent.mouseUp(window, { clientX: x, clientY: y });
    const wholeScanRequests = () =>
      ipc.developRender.mock.calls.filter(
        (c) =>
          JSON.stringify((c[1] as { geometry?: unknown }).geometry) ===
          JSON.stringify(WHOLE_SCAN_GEOMETRY),
      );
    const savedRecipe = () => state.saved[state.saved.length - 1]?.recipe;
    const pickButton = () => screen.getByRole('button', { name: 'Pick film base' });

    /** Opens frame 1 and turns picking on, with the whole scan on screen. */
    async function startPicking() {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(<DevelopView />);
      await canvas();
      await user.click(pickButton());
      expect(await screen.findByText(/Click the clear film edge/)).toBeInTheDocument();
      return user;
    }

    describe('the Geometry panel', () => {
      const cropArea = () => screen.getByRole('application', { name: /Crop area/ });
      const cropOf = (recipe: unknown) =>
        (recipe as { geometry: { crop: { x: number; y: number; w: number; h: number } } }).geometry
          .crop;

      async function openFrame() {
        const user = userEvent.setup();
        openLibrary();
        open(1);
        render(<DevelopView />);
        await canvas();
        // The analysis has to be in, for the detected frame.
        await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalled());
        return user;
      }

      it('turns the picture a quarter, previews it and saves it', async () => {
        const user = await openFrame();
        ipc.developRender.mockClear();
        await user.click(screen.getByRole('button', { name: 'Rotate right' }));
        await waitFor(() => expect(ipc.developRender).toHaveBeenCalled());
        expect(ipc.developRender.mock.calls.at(-1)?.[1]).toMatchObject({ geometry: { rot90: 2 } });
        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(1), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(savedRecipe()).toMatchObject({ geometry: { rot90: 2, angle_deg: 1.5 } });
      });

      it('flips and straightens, and shows a grid while the straighten slider is in use', async () => {
        const user = await openFrame();
        expect(document.querySelector('[class*="guide"]')).not.toBeInTheDocument();
        await user.click(screen.getByRole('button', { name: 'Flip vertically' }));
        const straighten = screen.getByRole('slider', { name: 'Straighten' });
        straighten.focus();
        await waitFor(() => expect(document.querySelector('[class*="guide"]')).toBeInTheDocument());
        await user.keyboard('{ArrowRight}');
        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalled(), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(savedRecipe()).toMatchObject({
          geometry: { flip_v: true, flip_h: true, angle_deg: expect.closeTo(1.6, 9) },
        });
        await user.tab();
        await waitFor(() =>
          expect(document.querySelector('[class*="guide"]')).not.toBeInTheDocument(),
        );
      });

      describe('editing the crop', () => {
        it('shows the whole scan with the crop over it, and draws the edit again afterwards', async () => {
          const user = await openFrame();
          await user.click(screen.getByRole('button', { name: 'Edit crop' }));
          expect(await screen.findByText(/Drag the frame or its sides/)).toBeInTheDocument();
          expect(screen.getByRole('img', { name: 'The whole scan, for cropping' })).toBeVisible();
          expect(cropArea()).toHaveStyle({ left: '10%', top: '10%', width: '80%', height: '80%' });
          expect(screen.getByRole('button', { name: 'Edit crop' })).toHaveAttribute(
            'aria-pressed',
            'true',
          );

          // Moving it draws nothing: the picture on screen is the whole scan.
          ipc.developRender.mockClear();
          cropArea().focus();
          await user.keyboard('{ArrowRight}{ArrowRight}');
          expect(cropArea()).toHaveStyle({ left: '11%' });
          expect(ipc.developRender).not.toHaveBeenCalled();
          await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(1), {
            timeout: SAVE_DELAY_MS * 4,
          });
          expect(cropOf(savedRecipe()).x).toBeCloseTo(0.11, 9);

          // Leaving crop mode draws the edit once, with the new crop.
          await user.keyboard('{Escape}');
          await canvasNamed('Frame 1');
          await waitFor(() => expect(ipc.developRender).toHaveBeenCalledTimes(1));
          expect(cropOf(ipc.developRender.mock.calls[0]?.[1]).x).toBeCloseTo(0.11, 9);
          expect(useUiStore.getState().view).toBe('develop');
          expect(screen.queryByRole('application')).not.toBeInTheDocument();
        });

        it('moves the crop by dragging it, and saves where it was dropped', async () => {
          const user = await openFrame();
          await user.click(screen.getByRole('button', { name: 'Edit crop' }));
          await screen.findByText(/Drag the frame or its sides/);
          // The picture is 400 x 300 at (100, 50): its middle is (300, 200).
          fireEvent.pointerDown(cropArea(), {
            button: 0,
            pointerId: 1,
            clientX: 300,
            clientY: 200,
          });
          fireEvent.pointerMove(cropArea(), { pointerId: 1, clientX: 320, clientY: 215 });
          fireEvent.pointerUp(cropArea(), { pointerId: 1 });
          await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalled(), {
            timeout: SAVE_DELAY_MS * 4,
          });
          const crop = cropOf(savedRecipe());
          expect(crop.x).toBeCloseTo(0.15, 6);
          expect(crop.y).toBeCloseTo(0.15, 6);
          expect(crop.w).toBeCloseTo(0.8, 6);
        });

        it('resizes it with the sliders for its sides', async () => {
          const user = await openFrame();
          const bottom = screen.getByRole('slider', { name: 'Bottom side' });
          bottom.focus();
          await user.keyboard('{PageDown}');
          await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalled(), {
            timeout: SAVE_DELAY_MS * 4,
          });
          const crop = cropOf(savedRecipe());
          expect(crop.y + crop.h).toBeCloseTo(0.89, 9);
          expect(crop.x).toBeCloseTo(0.1, 9);
        });

        it('stops when another frame is opened', async () => {
          const user = await openFrame();
          await user.click(screen.getByRole('button', { name: 'Edit crop' }));
          await screen.findByText(/Drag the frame or its sides/);
          await user.keyboard(']');
          await canvasNamed('Frame 2');
          expect(screen.getByRole('button', { name: 'Edit crop' })).toHaveAttribute(
            'aria-pressed',
            'false',
          );
        });

        it('has no accessibility violations while cropping', async () => {
          const user = await openFrame();
          await user.click(screen.getByRole('button', { name: 'Edit crop' }));
          await screen.findByText(/Drag the frame or its sides/);
          await expectNoA11yViolations(document.body);
        });
      });

      describe('locking the crop to an aspect ratio', () => {
        // The fake frame is 5616 x 3744 pixels (3:2) and its recipe is turned a quarter.
        const ratioSelect = () => screen.getByRole('combobox', { name: 'Aspect ratio' });
        const finishedRatio = (recipe: unknown) => {
          const g = (
            recipe as {
              geometry: { rot90: number; crop: { w: number; h: number } };
            }
          ).geometry;
          const inScan = (g.crop.w * 5616) / (g.crop.h * 3744);
          return g.rot90 % 2 === 0 ? inScan : 1 / inScan;
        };

        async function openFrame() {
          const user = userEvent.setup();
          openLibrary();
          open(1);
          render(<DevelopView />);
          await canvas();
          await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalled());
          return user;
        }

        it('fits the largest crop of the ratio for the finished picture, and saves it', async () => {
          const user = await openFrame();
          expect(ratioSelect()).toHaveValue('free');
          await user.selectOptions(ratioSelect(), '4:3');
          await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalled(), {
            timeout: SAVE_DELAY_MS * 4,
          });
          const crop = cropOf(savedRecipe());
          // Turned a quarter, 4:3 is 3:4 in the scan: half the width, all the height.
          expect(crop.w).toBeCloseTo(0.5, 9);
          expect(crop.h).toBeCloseTo(1, 9);
          expect(crop.x).toBeCloseTo(0.25, 9);
          expect(finishedRatio(savedRecipe())).toBeCloseTo(4 / 3, 9);
          expect(screen.getByText(/keeps the ratio 4:3/)).toBeInTheDocument();
        });

        it('keeps the ratio when the crop is resized by dragging a corner', async () => {
          const user = await openFrame();
          await user.selectOptions(ratioSelect(), '4:3');
          await user.click(screen.getByRole('button', { name: 'Edit crop' }));
          await screen.findByText(/Drag the frame or its sides/);
          const grip = document.querySelector('[data-handle="se"]') as HTMLElement;
          // The crop is {0.25, 0, 0.5, 1} on a picture of 400 x 300 at (100, 50): its corner is
          // at (400, 350). Drag it up and in.
          fireEvent.pointerDown(grip, { button: 0, pointerId: 1, clientX: 400, clientY: 350 });
          fireEvent.pointerMove(grip, { pointerId: 1, clientX: 320, clientY: 200 });
          fireEvent.pointerUp(grip, { pointerId: 1 });
          // The ratio itself was saved before the drag on a slow machine: wait for the drag's save.
          await waitFor(() => expect(cropOf(savedRecipe()).w).toBeCloseTo(0.3, 6), {
            timeout: SAVE_DELAY_MS * 4,
          });
          const crop = cropOf(savedRecipe());
          expect(crop.x).toBeCloseTo(0.25, 6);
          expect(crop.w).toBeCloseTo(0.3, 6);
          expect(crop.h).toBeCloseTo(0.6, 6);
          expect(finishedRatio(savedRecipe())).toBeCloseTo(4 / 3, 6);
        });

        it('keeps the finished picture’s ratio when it is turned', async () => {
          const user = await openFrame();
          await user.selectOptions(ratioSelect(), '4:3');
          await user.click(screen.getByRole('button', { name: 'Rotate right' }));
          await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalled(), {
            timeout: SAVE_DELAY_MS * 4,
          });
          expect(savedRecipe()).toMatchObject({ geometry: { rot90: 2 } });
          expect(finishedRatio(savedRecipe())).toBeCloseTo(4 / 3, 9);
        });

        it('swaps to portrait', async () => {
          const user = await openFrame();
          await user.selectOptions(ratioSelect(), '3:2');
          await user.click(screen.getByRole('button', { name: 'Swap landscape and portrait' }));
          await waitFor(() => expect(screen.getByText(/keeps the ratio 2:3/)).toBeInTheDocument());
          await waitFor(() => expect(finishedRatio(savedRecipe())).toBeCloseTo(2 / 3, 9), {
            timeout: SAVE_DELAY_MS * 4,
          });
        });

        it('is free again when another frame is opened', async () => {
          const user = await openFrame();
          await user.selectOptions(ratioSelect(), '1:1');
          expect(ratioSelect()).toHaveValue('1:1');
          // The select has focus, and keys are left to it; let go of it first.
          (document.activeElement as HTMLElement).blur();
          await user.keyboard(']');
          await canvasNamed('Frame 2');
          expect(ratioSelect()).toHaveValue('free');
          expect(screen.queryByText(/keeps the ratio/)).not.toBeInTheDocument();
        });

        it('has no accessibility violations with a ratio locked', async () => {
          const user = await openFrame();
          await user.selectOptions(ratioSelect(), 'custom');
          await expectNoA11yViolations(document.body);
        });
      });

      describe('the detected frame', () => {
        it('goes back to it from a crop that was changed', async () => {
          const user = await openFrame();
          await user.click(screen.getByRole('button', { name: 'Use the detected frame' }));
          await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalled(), {
            timeout: SAVE_DELAY_MS * 4,
          });
          expect(cropOf(savedRecipe())).toEqual({ x: 0.05, y: 0.1, w: 0.9, h: 0.8 });
          expect(screen.getByRole('button', { name: 'Use the detected frame' })).toBeDisabled();
          expect(screen.queryByRole('note')).not.toBeInTheDocument();
        });

        it('says when it could not be found reliably', async () => {
          state.frameConfidence = 0.3;
          await openFrame();
          expect(
            await screen.findByText(/The edges of the frame could not be found reliably/),
          ).toBeInTheDocument();
          expect(screen.getByRole('button', { name: 'Use the detected frame' })).toBeDisabled();
          expect(screen.getByRole('button', { name: 'Use the whole scan' })).toBeEnabled();
        });
      });
    });

    describe('the Sharpen panel', () => {
      const sharpenOf = (recipe: unknown) =>
        (recipe as { output: { sharpen: { amount: number; radius_px: number } } }).output.sharpen;

      async function openFrame() {
        const user = userEvent.setup();
        openLibrary();
        open(1);
        render(<DevelopView />);
        await canvas();
        return user;
      }

      it('saves the amount and the radius with the recipe, and draws nothing new', async () => {
        const user = await openFrame();
        ipc.developRender.mockClear();
        screen.getByRole('slider', { name: 'Amount' }).focus();
        await user.keyboard('{PageUp}');
        screen.getByRole('slider', { name: 'Radius' }).focus();
        await user.keyboard('{ArrowRight}');
        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalled(), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(sharpenOf(savedRecipe())).toEqual({ amount: 10, radius_px: 1.1 });
        // Sharpening is applied at export, so the picture on screen is not asked for again.
        expect(ipc.developRender).not.toHaveBeenCalled();
        expect(screen.getByRole('slider', { name: 'Amount' })).toHaveAttribute(
          'aria-valuenow',
          '10',
        );
      });

      it('resets to off, at a radius of one pixel', async () => {
        const user = await openFrame();
        screen.getByRole('slider', { name: 'Amount' }).focus();
        await user.keyboard('{End}');
        const reset = screen.getByRole('button', { name: 'Reset sharpening' });
        await waitFor(() => expect(reset).toBeEnabled());
        await user.click(reset);
        await waitFor(() => expect(sharpenOf(savedRecipe())).toEqual({ amount: 0, radius_px: 1 }), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(screen.getByRole('slider', { name: 'Amount' })).toHaveAttribute(
          'aria-valuenow',
          '0',
        );
      });

      it('is out of reach in a library that cannot be changed', async () => {
        openLibrary({ read_only: true });
        open(1);
        render(<DevelopView />);
        await canvas();
        expect(screen.getByRole('slider', { name: 'Amount' })).toHaveAttribute('data-disabled');
        expect(screen.getByRole('button', { name: 'Reset sharpening' })).toBeDisabled();
      });

      it('has no accessibility violations', async () => {
        await openFrame();
        await expectNoA11yViolations(document.body);
      });
    });

    describe('the Dust panel', () => {
      const dustOf = (recipe: unknown) =>
        (recipe as { dust: { enabled: boolean; threshold: number; grow_px: number } }).dust;

      async function openFrame(hasIr = true) {
        state.hasIr = hasIr;
        const user = userEvent.setup();
        openLibrary();
        open(1);
        render(<DevelopView />);
        await canvas();
        return user;
      }

      it('is there only for a frame with an infrared plane', async () => {
        await openFrame(false);
        expect(screen.queryByRole('heading', { name: 'Dust removal' })).not.toBeInTheDocument();
        expect(screen.queryByRole('slider', { name: 'Sensitivity' })).not.toBeInTheDocument();
      });

      it('saves a changed sensitivity and growth, and draws the picture again', async () => {
        const user = await openFrame();
        expect(screen.getByRole('heading', { name: 'Dust removal' })).toBeInTheDocument();
        ipc.developRender.mockClear();
        screen.getByRole('slider', { name: 'Sensitivity' }).focus();
        await user.keyboard('{ArrowRight}');
        screen.getByRole('slider', { name: 'Widen spots by' }).focus();
        await user.keyboard('{ArrowRight}');
        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalled(), {
          timeout: SAVE_DELAY_MS * 4,
        });
        await waitFor(() => expect(dustOf(savedRecipe())).toMatchObject({ grow_px: 3 }), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(dustOf(savedRecipe())).toEqual({ enabled: true, threshold: 0.49, grow_px: 3 });
        // Dust removal changes the picture, so the preview is asked for again.
        await waitFor(() => expect(ipc.developRender).toHaveBeenCalled());
      });

      it('switches off and saves it', async () => {
        const user = await openFrame();
        await user.click(screen.getByRole('checkbox', { name: 'Remove dust' }));
        await waitFor(() => expect(dustOf(savedRecipe()).enabled).toBe(false), {
          timeout: SAVE_DELAY_MS * 4,
        });
      });

      it('says when dust removal is switched off in Settings', async () => {
        state.flags.dust_removal = false;
        await openFrame();
        expect(await screen.findByText(/switched off in Settings/)).toBeInTheDocument();
      });

      it('is out of reach in a library that cannot be changed', async () => {
        state.hasIr = true;
        openLibrary({ read_only: true });
        open(1);
        render(<DevelopView />);
        await canvas();
        expect(screen.getByRole('checkbox', { name: 'Remove dust' })).toBeDisabled();
        expect(screen.getByRole('slider', { name: 'Sensitivity' })).toHaveAttribute(
          'data-disabled',
        );
      });

      it('has no accessibility violations', async () => {
        await openFrame();
        await expectNoA11yViolations(document.body);
      });
    });
    describe('the Tone panel', () => {
      async function openFrame() {
        const user = userEvent.setup();
        openLibrary();
        open(1);
        render(<DevelopView />);
        await canvas();
        return user;
      }

      it('previews and saves a tone slider moved with the keyboard', async () => {
        const user = await openFrame();
        ipc.developRender.mockClear();
        screen.getByRole('slider', { name: 'Contrast' }).focus();
        await user.keyboard('{PageUp}');
        await waitFor(() => expect(ipc.developRender).toHaveBeenCalled());
        expect(ipc.developRender.mock.calls.at(-1)?.[1]).toMatchObject({
          tone: { contrast: 10, exposure_ev: 0 },
        });
        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(1), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(savedRecipe()).toMatchObject({ tone: { contrast: 10 } });
      });

      it('previews and saves a curve edited with the keyboard, then resets it', async () => {
        const user = await openFrame();
        ipc.developRender.mockClear();
        await user.click(screen.getByRole('button', { name: 'Add point' }));
        await waitFor(() => expect(ipc.developRender).toHaveBeenCalled());
        const point = await screen.findByRole('slider', { name: 'Point 2 of 3' });
        expect(point).toHaveFocus();
        await user.keyboard('{ArrowUp}{ArrowUp}');
        await waitFor(() =>
          expect(ipc.developRender.mock.calls.at(-1)?.[1]).toMatchObject({
            tone: {
              curve: {
                master: [
                  [0, 0],
                  [0.5, 0.52],
                  [1, 1],
                ],
              },
            },
          }),
        );
        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalled(), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(savedRecipe()).toMatchObject({
          tone: {
            curve: {
              master: [
                [0, 0],
                [0.5, 0.52],
                [1, 1],
              ],
            },
          },
        });

        await user.click(screen.getByRole('button', { name: 'Reset curve' }));
        expect(await screen.findAllByRole('slider', { name: /^Point \d of 2$/ })).toHaveLength(2);
      });

      it('edits a colour curve on its own, leaving the master curve as it is', async () => {
        const user = await openFrame();
        await user.click(screen.getByRole('button', { name: 'Red' }));
        await user.click(screen.getByRole('button', { name: 'Add point' }));
        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalled(), {
          timeout: SAVE_DELAY_MS * 4,
        });
        const tone = (savedRecipe() as unknown as { tone: { curve: Record<string, number[][]> } })
          .tone;
        expect(tone.curve.r).toHaveLength(3);
        expect(tone.curve.master).toEqual([
          [0, 0],
          [1, 1],
        ]);
      });

      it('has no accessibility violations', async () => {
        await openFrame();
        await expectNoA11yViolations(document.body);
      });
    });

    describe('the Colour panel', () => {
      const neutralButton = () => screen.getByRole('button', { name: 'Pick neutral grey' });
      const slider = (name: string) => screen.getByRole('slider', { name });

      async function openFrame() {
        const user = userEvent.setup();
        openLibrary();
        open(1);
        render(<DevelopView />);
        await canvas();
        return user;
      }

      it('previews and saves a colour slider moved with the keyboard', async () => {
        const user = await openFrame();
        ipc.developRender.mockClear();
        slider('Temperature').focus();
        await user.keyboard('{ArrowRight}');
        await waitFor(() => expect(ipc.developRender).toHaveBeenCalled());
        expect(ipc.developRender.mock.calls.at(-1)?.[1]).toMatchObject({
          color: { temperature: 1 },
        });
        expect(slider('Temperature')).toHaveAttribute('aria-valuenow', '1');
        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(1), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(savedRecipe()).toMatchObject({ color: { temperature: 1, saturation: 1 } });
      });

      it('shows the frame without turns while picking, and sets the axes from a click', async () => {
        const user = await openFrame();
        await user.click(neutralButton());
        expect(await screen.findByText(/part of the picture that should be grey/)).toBeVisible();
        expect(neutralButton()).toHaveAttribute('aria-pressed', 'true');
        expect(
          screen.getByRole('img', {
            name: 'The frame without turns or straightening, for picking on it',
          }),
        ).toBeInTheDocument();

        press(300, 200);
        release(300, 200);
        await waitFor(() => expect(ipc.developSampleNeutral).toHaveBeenCalledTimes(1));
        const [frameId, , region] = ipc.developSampleNeutral.mock.calls[0] as [
          string,
          never,
          { x: number; y: number; w: number; h: number },
        ];
        expect(frameId).toBe('summer-f00001');
        expect(region.x + region.w / 2).toBeCloseTo(0.5, 6);

        expect(await screen.findByText('The colour balance was set from that area.')).toBeVisible();
        expect(slider('Cyan / red')).toHaveAttribute('aria-valuenow', '-20');
        expect(slider('Magenta / green')).toHaveAttribute('aria-valuenow', '10');
        expect(slider('Yellow / blue')).toHaveAttribute('aria-valuenow', '30');
        expect(neutralButton()).toHaveAttribute('aria-pressed', 'false');
        await canvasNamed('Frame 1');
        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(1), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(savedRecipe()).toMatchObject({ color: { red_cyan: -20, blue_yellow: 30 } });
      });

      it('says when the area cannot be used, and keeps picking', async () => {
        state.neutralRefused = 'That area is too far from neutral to correct.';
        const user = await openFrame();
        await user.click(neutralButton());
        await screen.findByText(/part of the picture that should be grey/);
        press(300, 200);
        release(300, 200);
        expect(await screen.findByRole('alert')).toHaveTextContent(
          'That area could not be used: That area is too far from neutral to correct.',
        );
        expect(neutralButton()).toHaveAttribute('aria-pressed', 'true');
        expect(ipc.recipeSave).not.toHaveBeenCalled();
      });

      it('is one pick at a time, and Escape cancels it', async () => {
        const user = await openFrame();
        await user.click(screen.getByRole('button', { name: 'Pick white point' }));
        await screen.findByText(/brightest part/);
        await user.click(neutralButton());
        expect(neutralButton()).toHaveAttribute('aria-pressed', 'true');
        expect(screen.getByRole('button', { name: 'Pick white point' })).toHaveAttribute(
          'aria-pressed',
          'false',
        );
        await user.keyboard('{Escape}');
        expect(neutralButton()).toHaveAttribute('aria-pressed', 'false');
        expect(useUiStore.getState().view).toBe('develop');
        expect(ipc.developSampleNeutral).not.toHaveBeenCalled();
      });

      it('switches to black and white, which hides saturation and vibrance and saves', async () => {
        const user = await openFrame();
        expect(slider('Saturation')).toBeInTheDocument();
        await user.click(screen.getByRole('checkbox', { name: 'Black and white' }));
        expect(await screen.findByRole('group', { name: 'Channel mix' })).toBeInTheDocument();
        expect(screen.queryByRole('slider', { name: 'Saturation' })).not.toBeInTheDocument();
        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(1), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(savedRecipe()).toMatchObject({ mono: { enabled: true } });

        await user.click(screen.getByRole('checkbox', { name: 'Black and white' }));
        expect(await screen.findByRole('slider', { name: 'Saturation' })).toBeInTheDocument();
      });

      it('has no accessibility violations while picking', async () => {
        const user = await openFrame();
        await user.click(neutralButton());
        await screen.findByText(/part of the picture that should be grey/);
        await expectNoA11yViolations(document.body);
      });
    });

    describe('picking the white and black points', () => {
      const frameRequests = () =>
        ipc.developRender.mock.calls.filter((c) => {
          const g = (c[1] as { geometry?: { rot90?: number } }).geometry;
          return g?.rot90 === 0 && JSON.stringify(g) !== JSON.stringify(WHOLE_SCAN_GEOMETRY);
        });

      /** Opens frame 1 and turns picking of the white or black point on. */
      async function startLevelPicking(name: string, hint: RegExp) {
        const user = userEvent.setup();
        openLibrary();
        open(1);
        render(<DevelopView />);
        await canvas();
        await user.click(screen.getByRole('button', { name }));
        expect(await screen.findByText(hint)).toBeInTheDocument();
        return user;
      }

      it('shows the frame without turns, sets the white point from a click, and saves it', async () => {
        await startLevelPicking('Pick white point', /brightest part/);
        expect(frameRequests()).toHaveLength(1);
        expect(
          screen.getByRole('img', {
            name: 'The frame without turns or straightening, for picking on it',
          }),
        ).toBeInTheDocument();

        press(300, 200);
        release(300, 200);
        await waitFor(() => expect(ipc.developSampleLevels).toHaveBeenCalledTimes(1));
        const [frameId, recipe, region, point] = ipc.developSampleLevels.mock.calls[0] as [
          string,
          never,
          { x: number; y: number; w: number; h: number },
          string,
        ];
        expect([frameId, point]).toEqual(['summer-f00001', 'white']);
        expect(levelsOf(recipe).white).toEqual([0.9, 0.8, 0.7]);
        expect(region.x + region.w / 2).toBeCloseTo(0.5, 6);
        expect(region.y + region.h / 2).toBeCloseTo(0.5, 6);

        // The white point is what was measured, the black point is kept, and picking is over.
        expect(await screen.findByText('The white point was set from that area.')).toBeVisible();
        const sliderValue = (name: string) =>
          screen.getByRole('slider', { name }).getAttribute('aria-valuenow');
        expect(sliderValue('White point, red')).toBe('0.6');
        expect(sliderValue('White point, blue')).toBe('0.8');
        expect(sliderValue('Black point, red')).toBe('0.1');
        expect(screen.getByRole('button', { name: 'Pick white point' })).toHaveAttribute(
          'aria-pressed',
          'false',
        );
        await canvasNamed('Frame 1');

        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(1), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(levelsOf(savedRecipe() as never).white).toEqual([0.6, 0.7, 0.8]);
      });

      it('sets the black point from a box dragged on the frame', async () => {
        await startLevelPicking('Pick black point', /darkest part/);
        press(200, 100);
        fireEvent.mouseMove(window, { clientX: 300, clientY: 200 });
        release(300, 200);
        await waitFor(() => expect(ipc.developSampleLevels).toHaveBeenCalledTimes(1));
        expect(ipc.developSampleLevels.mock.calls[0]?.[2]).toEqual({
          x: 0.25,
          y: 50 / 300,
          w: 0.25,
          h: 100 / 300,
        });
        expect(ipc.developSampleLevels.mock.calls[0]?.[3]).toBe('black');
        expect(
          await screen.findByText('The black point was set from that area.'),
        ).toBeInTheDocument();
        expect(
          screen.getByRole('slider', { name: 'Black point, green' }).getAttribute('aria-valuenow'),
        ).toBe('0.03');
      });

      it('says when the area cannot be used, and keeps picking', async () => {
        state.levelsRefused = 'That area is black, so it says nothing about the levels.';
        await startLevelPicking('Pick white point', /brightest part/);
        press(300, 200);
        release(300, 200);
        expect(await screen.findByRole('alert')).toHaveTextContent(
          'That area could not be used: That area is black, so it says nothing about the levels.',
        );
        expect(screen.getByRole('button', { name: 'Pick white point' })).toHaveAttribute(
          'aria-pressed',
          'true',
        );
        expect(ipc.recipeSave).not.toHaveBeenCalled();
      });

      it('switches from one pick to another, and is cancelled by Escape', async () => {
        const user = await startLevelPicking('Pick white point', /brightest part/);
        await user.click(screen.getByRole('button', { name: 'Pick black point' }));
        expect(await screen.findByText(/darkest part/)).toBeInTheDocument();
        expect(screen.getByRole('button', { name: 'Pick white point' })).toHaveAttribute(
          'aria-pressed',
          'false',
        );
        // Still the frame view: the picture was not asked for again.
        expect(frameRequests()).toHaveLength(1);

        await user.keyboard('{Escape}');
        expect(screen.getByRole('button', { name: 'Pick black point' })).toHaveAttribute(
          'aria-pressed',
          'false',
        );
        expect(useUiStore.getState().view).toBe('develop');
        await canvasNamed('Frame 1');
        expect(ipc.developSampleLevels).not.toHaveBeenCalled();
      });

      it('has no accessibility violations while picking', async () => {
        await startLevelPicking('Pick black point', /darkest part/);
        await expectNoA11yViolations(document.body);
      });
    });

    describe('picking the film base', () => {
      it('shows the whole scan while picking, sets a base from a click, and saves it', async () => {
        await startPicking();
        expect(wholeScanRequests()).toHaveLength(1);
        expect(
          screen.getByRole('img', { name: 'The whole scan, for picking the film base' }),
        ).toBeInTheDocument();
        expect(pickButton()).toHaveAttribute('aria-pressed', 'true');

        // A click in the middle of the picture: a small square around the middle of the scan.
        press(300, 200);
        release(300, 200);
        await waitFor(() => expect(ipc.developSampleBase).toHaveBeenCalledTimes(1));
        const [frameId, region] = ipc.developSampleBase.mock.calls[0] as [
          string,
          { x: number; y: number; w: number; h: number },
        ];
        expect(frameId).toBe('summer-f00001');
        expect(region.x + region.w / 2).toBeCloseTo(0.5, 6);
        expect(region.y + region.h / 2).toBeCloseTo(0.5, 6);

        // The frame now has its own base, picking is over, and the picture is back.
        expect(await screen.findByText('Picked on this frame.')).toBeInTheDocument();
        expect(screen.getByText('Red 0.800, green 0.500, blue 0.300')).toBeInTheDocument();
        expect(screen.getByText('Film base picked from that area.')).toBeInTheDocument();
        expect(pickButton()).toHaveAttribute('aria-pressed', 'false');
        await canvasNamed('Frame 1');

        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(1), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(baseOf(savedRecipe() as never)).toEqual({ source: 'manual', rgb: [0.8, 0.5, 0.3] });
        expect(await screen.findByText('Saved')).toBeInTheDocument();
      });

      it('sets the base from a box dragged on the rebate, and shows the box while it is dragged', async () => {
        await startPicking();
        press(200, 100);
        fireEvent.mouseMove(window, { clientX: 300, clientY: 200 });
        expect(document.querySelector('[class*="pickBox"]')).toBeInTheDocument();
        release(300, 200);
        await waitFor(() => expect(ipc.developSampleBase).toHaveBeenCalledTimes(1));
        expect(ipc.developSampleBase.mock.calls[0]?.[1]).toEqual({
          x: 0.25,
          y: 50 / 300,
          w: 0.25,
          h: 100 / 300,
        });
        expect(document.querySelector('[class*="pickBox"]')).not.toBeInTheDocument();
      });

      it('ignores a press outside the picture', async () => {
        await startPicking();
        press(20, 20);
        release(20, 20);
        expect(ipc.developSampleBase).not.toHaveBeenCalled();
        expect(pickButton()).toHaveAttribute('aria-pressed', 'true');
      });

      it('says when the area cannot be used, and keeps picking', async () => {
        state.sampledBase = null;
        await startPicking();
        press(300, 200);
        release(300, 200);
        expect(await screen.findByRole('alert')).toHaveTextContent(
          'That area could not be used: That area is too dark to be film base.',
        );
        expect(pickButton()).toHaveAttribute('aria-pressed', 'true');
        expect(screen.getByText(/The roll has no film base yet/)).toBeInTheDocument();
        expect(ipc.recipeSave).not.toHaveBeenCalled();
      });

      it('is cancelled by Escape, which leaves the view only when pressed again', async () => {
        const user = await startPicking();
        await user.keyboard('{Escape}');
        expect(pickButton()).toHaveAttribute('aria-pressed', 'false');
        expect(useUiStore.getState().view).toBe('develop');
        await canvasNamed('Frame 1');

        await user.keyboard('{Escape}');
        expect(useUiStore.getState().view).toBe('library');
      });

      it('stops when another frame is opened', async () => {
        const user = await startPicking();
        await user.keyboard(']');
        await canvasNamed('Frame 2');
        expect(pickButton()).toHaveAttribute('aria-pressed', 'false');
      });

      it('goes back to the roll’s base, and the roll’s default is set from the picked one', async () => {
        const user = await startPicking();
        press(300, 200);
        release(300, 200);
        await screen.findByText('Picked on this frame.');

        await user.click(screen.getByRole('button', { name: 'Use as the roll’s default' }));
        await waitFor(() => expect(ipc.rollSetFilmBase).toHaveBeenCalledTimes(1));
        expect(ipc.rollSetFilmBase).toHaveBeenCalledWith('summer', [0.8, 0.5, 0.3]);
        // The frame is read again, so what follows the roll's base uses the new one.
        await waitFor(() => expect(ipc.developOpen.mock.calls.length).toBeGreaterThanOrEqual(2));
        expect(
          await screen.findByText('This film base is now the roll’s default.'),
        ).toBeInTheDocument();

        await user.click(screen.getByRole('button', { name: 'Use the roll’s base' }));
        expect(
          await screen.findByText('Following the roll’s film base.', { exact: false }),
        ).toBeInTheDocument();
        await waitFor(() => expect(baseOf(savedRecipe() as never).source).toBe('roll'), {
          timeout: SAVE_DELAY_MS * 4,
        });
      });

      it('says when the roll’s base could not be set', async () => {
        const user = await startPicking();
        press(300, 200);
        release(300, 200);
        await screen.findByText('Picked on this frame.');
        ipc.rollSetFilmBase.mockRejectedValueOnce(new Error('The roll is gone.'));
        await user.click(screen.getByRole('button', { name: 'Use as the roll’s default' }));
        expect(await screen.findByRole('alert')).toHaveTextContent(
          'The roll’s film base could not be set: The roll is gone.',
        );
      });
    });

    describe('the levels', () => {
      it('re-runs the automatic levels and saves them', async () => {
        const user = userEvent.setup();
        openLibrary();
        open(1);
        render(<DevelopView />);
        await canvas();
        await user.click(screen.getByRole('button', { name: 'Re-run auto levels' }));

        await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalledWith('summer-f00001', null));
        expect(
          await screen.findByText('Levels set automatically for this film base.'),
        ).toBeInTheDocument();
        expect(screen.getByRole('slider', { name: 'Black point, red' })).toHaveAttribute(
          'aria-valuenow',
          '0.05',
        );
        expect(screen.getByRole('slider', { name: 'White point, blue' })).toHaveAttribute(
          'aria-valuenow',
          '0.75',
        );
        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(1), {
          timeout: SAVE_DELAY_MS * 4,
        });
        expect(levelsOf(savedRecipe() as never)).toEqual({
          black: [0.05, 0.06, 0.07],
          white: [0.95, 0.85, 0.75],
        });
      });

      it('fits the levels to the roll’s base, or to the base picked on the frame', async () => {
        state.rollBase = [0.7, 0.5, 0.4];
        const user = userEvent.setup();
        openLibrary();
        open(1);
        render(<DevelopView />);
        await canvas();
        await user.click(screen.getByRole('button', { name: 'Re-run auto levels' }));
        await waitFor(() =>
          expect(ipc.developAnalyze).toHaveBeenLastCalledWith('summer-f00001', [0.7, 0.5, 0.4]),
        );

        // Once a base has been picked, that is the one the levels are fitted to.
        await user.click(pickButton());
        await screen.findByText(/Click the clear film edge/);
        press(300, 200);
        release(300, 200);
        await screen.findByText('Picked on this frame.');
        await user.click(screen.getByRole('button', { name: 'Re-run auto levels' }));
        await waitFor(() =>
          expect(ipc.developAnalyze).toHaveBeenLastCalledWith('summer-f00001', [0.8, 0.5, 0.3]),
        );
        expect(screen.getByRole('slider', { name: 'White point, red' })).toHaveAttribute(
          'aria-valuenow',
          '0.5',
        );
      });

      it('says when the automatic levels cannot be found, and changes nothing', async () => {
        const user = userEvent.setup();
        openLibrary();
        open(1);
        render(<DevelopView />);
        await canvas();
        await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalledTimes(1));
        ipc.developAnalyze.mockRejectedValueOnce(new Error('The scan could not be analysed.'));
        await user.click(screen.getByRole('button', { name: 'Re-run auto levels' }));
        expect(await screen.findByRole('alert')).toHaveTextContent(
          'The automatic levels could not be found: The scan could not be analysed.',
        );
        expect(screen.getByRole('slider', { name: 'Black point, red' })).toHaveAttribute(
          'aria-valuenow',
          '0.1',
        );
      });

      it('overrides the levels with a slider, shows a sharp picture at once, and saves', async () => {
        const user = userEvent.setup();
        openLibrary();
        open(1);
        render(<DevelopView />);
        await canvas();
        ipc.developRender.mockClear();

        screen.getByRole('slider', { name: 'White point, green' }).focus();
        await user.keyboard('{ArrowRight}');
        await waitFor(() => expect(ipc.developRender).toHaveBeenCalledTimes(1));
        expect(ipc.developRender.mock.calls[0]?.[2]).toBe(2048);
        expect(levelsOf(ipc.developRender.mock.calls[0]?.[1] as never).white).toEqual([
          0.9, 0.805, 0.7,
        ]);
        expect(screen.getByRole('slider', { name: 'White point, green' })).toHaveAttribute(
          'aria-valuenow',
          '0.805',
        );

        expect(
          await screen.findByText('Saved', undefined, { timeout: SAVE_DELAY_MS * 4 }),
        ).toBeInTheDocument();
        expect(levelsOf(savedRecipe() as never).white[1]).toBe(0.805);
      });
    });

    describe('saving', () => {
      it('says when an edit could not be saved', async () => {
        state.saveFails = 'The disk is full.';
        const user = userEvent.setup();
        openLibrary();
        open(1);
        render(<DevelopView />);
        await canvas();
        screen.getByRole('slider', { name: 'Black point, blue' }).focus();
        await user.keyboard('{ArrowRight}');
        expect(
          await screen.findByRole('alert', undefined, { timeout: SAVE_DELAY_MS * 4 }),
        ).toHaveTextContent('The edit could not be saved: The disk is full.');
      });

      it('saves an edit that is waiting when another frame is opened', async () => {
        const user = userEvent.setup();
        openLibrary();
        open(1);
        render(<DevelopView />);
        await canvas();
        await screen.findAllByRole('option');
        screen.getByRole('slider', { name: 'Black point, blue' }).focus();
        await user.keyboard('{ArrowRight}');
        expect(ipc.recipeSave).not.toHaveBeenCalled();

        await user.keyboard(']');
        await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(1));
        expect(ipc.recipeSave.mock.calls[0]?.[0]).toBe('summer-f00001');
        expect(levelsOf(ipc.recipeSave.mock.calls[0]?.[1] as never).black[2]).toBe(0.305);
      });
    });

    it('can be looked at but not used in a library from a newer version', async () => {
      openLibrary({ read_only: true });
      open(1);
      render(<DevelopView />);
      await canvas();
      expect(await screen.findByText(/This library cannot be changed/)).toBeInTheDocument();
      expect(pickButton()).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Re-run auto levels' })).toBeDisabled();
      for (const slider of screen.getAllByRole('slider')) {
        // A slider of the panels, or a point of a curve.
        const disabled =
          slider.hasAttribute('data-disabled') || slider.getAttribute('aria-disabled') === 'true';
        expect(disabled).toBe(true);
      }
      expect(screen.getByRole('button', { name: 'Pick neutral grey' })).toBeDisabled();
      expect(screen.getByRole('checkbox', { name: 'Black and white' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Add point' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Edit crop' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Rotate right' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Reset curve' })).toBeDisabled();
    });
  });
  describe('accessibility', () => {
    it('has no violations: empty, open, and showing "before"', async () => {
      const empty = render(<DevelopView />);
      await expectNoA11yViolations(empty.container);
      empty.unmount();

      const user = userEvent.setup();
      openLibrary();
      open(2);
      const { container } = render(<DevelopView />);
      await canvas();
      await screen.findAllByRole('option');
      await expectNoA11yViolations(container);
      await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalled());
      await user.click(screen.getByRole('button', { name: 'Before' }));
      await screen.findByText(/Showing the automatic conversion/);
      await expectNoA11yViolations(container);
    });

    it('has no violations while the film base is being picked', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(2);
      const { container } = render(<DevelopView />);
      await canvas();
      await screen.findAllByRole('option');
      await user.click(screen.getByRole('button', { name: 'Pick film base' }));
      await screen.findByText(/Click the clear film edge/);
      await expectNoA11yViolations(container);
    });
  });
});
