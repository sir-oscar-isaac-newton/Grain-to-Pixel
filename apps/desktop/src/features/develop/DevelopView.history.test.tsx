import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
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
import { levelsOf } from './recipe-parts';
import { MERGE_WINDOW_MS } from './history';
import { SAVE_DELAY_MS } from './use-develop';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

function openLibrary() {
  state.details = libraryDetails({});
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

const lastRender = () => ipc.developRender.mock.calls.at(-1)?.[1] as never;
const savedRecipe = () => state.saved[state.saved.length - 1]?.recipe as never;
const slider = (name: string) => screen.getByRole('slider', { name });
const value = (name: string) => Number(slider(name).getAttribute('aria-valuenow'));
const button = (name: string) => screen.getByRole('button', { name });

/** The black point of red: the saved recipe has 0.1, the automatic one 0.05. */
const BLACK_RED = 'Black point, red';

describe('Develop undo, redo and reset', () => {
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

  async function openFrame(options: { analysis?: boolean } = {}) {
    if (options.analysis === false) state.noAnalysis.add('summer-f00001');
    const user = userEvent.setup();
    openLibrary();
    useUiStore.setState({ view: 'develop', developFrame: { id: 'summer-f00001', index: 0 } });
    const view = render(<DevelopView />);
    await canvas();
    await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalled());
    if (options.analysis !== false) await waitFor(() => expect(button('Reset all')).toBeEnabled());
    return { user, ...view };
  }

  /** Moves the contrast slider one page up (10) with the keyboard: one step. */
  async function nudgeContrast(user: ReturnType<typeof userEvent.setup>) {
    slider('Contrast').focus();
    await user.keyboard('{PageUp}');
    await waitFor(() => expect(value('Contrast')).toBe(10));
  }

  describe('the keys', () => {
    it('undoes with Ctrl+Z and redoes with Ctrl+Shift+Z, saving each state', async () => {
      const { user } = await openFrame();
      await nudgeContrast(user);
      await user.keyboard('{Control>}z{/Control}');
      await waitFor(() => expect(value('Contrast')).toBe(0));
      await waitFor(() => expect(savedRecipe()).toMatchObject({ tag: 'saved' }), {
        timeout: SAVE_DELAY_MS * 4,
      });
      await user.keyboard('{Control>}{Shift>}z{/Shift}{/Control}');
      await waitFor(() => expect(value('Contrast')).toBe(10));
      expect(lastRender()).toMatchObject({ tone: { contrast: 10 } });
      await waitFor(() => expect(savedRecipe()).toMatchObject({ tone: { contrast: 10 } }), {
        timeout: SAVE_DELAY_MS * 4,
      });
    });

    it('also takes Cmd+Z, Cmd+Shift+Z and Ctrl+Y', async () => {
      const { user } = await openFrame();
      await nudgeContrast(user);
      await user.keyboard('{Meta>}z{/Meta}');
      await waitFor(() => expect(value('Contrast')).toBe(0));
      await user.keyboard('{Meta>}{Shift>}z{/Shift}{/Meta}');
      await waitFor(() => expect(value('Contrast')).toBe(10));
      await user.keyboard('{Control>}z{/Control}');
      await waitFor(() => expect(value('Contrast')).toBe(0));
      await user.keyboard('{Control>}y{/Control}');
      await waitFor(() => expect(value('Contrast')).toBe(10));
    });

    it.each([
      ['an input', () => document.createElement('input')],
      ['a textarea', () => document.createElement('textarea')],
      [
        'a select',
        () => {
          const select = document.createElement('select');
          select.appendChild(document.createElement('option'));
          return select;
        },
      ],
      [
        'an editable element',
        () => {
          const div = document.createElement('div');
          div.setAttribute('contenteditable', 'true');
          div.tabIndex = 0;
          return div;
        },
      ],
    ])('leaves the keys alone while the focus is in %s', async (_name, make) => {
      const { user } = await openFrame();
      await nudgeContrast(user);
      const field = make();
      document.body.appendChild(field);
      field.focus();
      fireEvent.keyDown(field, { key: 'z', ctrlKey: true });
      fireEvent.keyDown(field, { key: 'z', ctrlKey: true, shiftKey: true });
      expect(value('Contrast')).toBe(10);
      expect(button('Undo')).toBeEnabled();
      expect(button('Redo')).toBeDisabled();
      field.remove();
    });

    it('still undoes while a checkbox has the focus', async () => {
      const { user } = await openFrame();
      await user.click(screen.getByRole('checkbox', { name: 'Black and white' }));
      await user.keyboard('{Control>}z{/Control}');
      await waitFor(() =>
        expect(screen.getByRole('checkbox', { name: 'Black and white' })).not.toBeChecked(),
      );
    });
  });

  describe('the buttons', () => {
    it('are disabled until there is something to undo or redo', async () => {
      const { user } = await openFrame();
      expect(button('Undo')).toBeDisabled();
      expect(button('Redo')).toBeDisabled();
      await nudgeContrast(user);
      expect(button('Undo')).toBeEnabled();
      expect(button('Redo')).toBeDisabled();
      await user.click(button('Undo'));
      await waitFor(() => expect(value('Contrast')).toBe(0));
      expect(button('Undo')).toBeDisabled();
      expect(button('Redo')).toBeEnabled();
      await user.click(button('Redo'));
      await waitFor(() => expect(value('Contrast')).toBe(10));
      expect(button('Redo')).toBeDisabled();
    });

    it('drop the redo step when a new change follows an undo', async () => {
      const { user } = await openFrame();
      await nudgeContrast(user);
      await user.click(button('Undo'));
      expect(button('Redo')).toBeEnabled();
      slider('Contrast').focus();
      await user.keyboard('{ArrowRight}');
      await waitFor(() => expect(value('Contrast')).toBe(1));
      expect(button('Redo')).toBeDisabled();
    });

    it('make a change from the Colour keys one step too', async () => {
      const { user } = await openFrame();
      await user.keyboard('q');
      await waitFor(() => expect(button('Undo')).toBeEnabled());
      await user.click(button('Undo'));
      await waitFor(() => expect(value('Cyan / red')).toBe(0));
    });
  });

  describe('resetting a slider', () => {
    it('goes back to the automatic value on a double click, as one undo step', async () => {
      const { user } = await openFrame();
      expect(value(BLACK_RED)).toBe(0.1);
      fireEvent.doubleClick(slider(BLACK_RED));
      await waitFor(() => expect(value(BLACK_RED)).toBe(0.05));
      expect(levelsOf(lastRender()).black[0]).toBe(0.05);
      await waitFor(() => expect(levelsOf(savedRecipe()).black[0]).toBe(0.05), {
        timeout: SAVE_DELAY_MS * 4,
      });
      await user.click(button('Undo'));
      await waitFor(() => expect(value(BLACK_RED)).toBe(0.1));
      expect(button('Undo')).toBeDisabled();
    });

    it('goes back on a double click anywhere on the slider, the label and value included', async () => {
      await openFrame();
      fireEvent.doubleClick(screen.getByText(BLACK_RED));
      await waitFor(() => expect(value(BLACK_RED)).toBe(0.05));
    });

    it('makes no step of its own when the value is already the automatic one', async () => {
      await openFrame();
      fireEvent.doubleClick(slider(BLACK_RED));
      await waitFor(() => expect(value(BLACK_RED)).toBe(0.05));
      fireEvent.doubleClick(slider(BLACK_RED));
      fireEvent.doubleClick(slider(BLACK_RED));
      expect(button('Undo')).toBeEnabled();
      await userEvent.click(button('Undo'));
      await waitFor(() => expect(value(BLACK_RED)).toBe(0.1));
      expect(button('Undo')).toBeDisabled();
    });

    it.each(['{Delete}', '{Backspace}'])(
      'goes back on %s while the thumb has the focus',
      async (key) => {
        const { user } = await openFrame();
        slider(BLACK_RED).focus();
        await user.keyboard(key);
        await waitFor(() => expect(value(BLACK_RED)).toBe(0.05));
        await user.click(button('Undo'));
        await waitFor(() => expect(value(BLACK_RED)).toBe(0.1));
      },
    );

    it('resets a tone slider to the automatic recipe, which has none', async () => {
      const { user } = await openFrame();
      await nudgeContrast(user);
      fireEvent.doubleClick(slider('Contrast'));
      await waitFor(() => expect(value('Contrast')).toBe(0));
    });

    describe('with a double click on the track', () => {
      /** A change of the slider (the first click of the double click), then the reset. */
      async function moveThenReset(user: ReturnType<typeof userEvent.setup>, name: string) {
        slider(name).focus();
        await user.keyboard('{PageUp}');
        await waitFor(() => expect(button('Undo')).toBeEnabled());
        fireEvent.doubleClick(slider(name));
      }

      it('is one undo step: undo goes back to before the double click, redo to the reset', async () => {
        const { user } = await openFrame();
        expect(value(BLACK_RED)).toBe(0.1);
        await moveThenReset(user, BLACK_RED);
        await waitFor(() => expect(value(BLACK_RED)).toBe(0.05));
        await user.click(button('Undo'));
        await waitFor(() => expect(value(BLACK_RED)).toBe(0.1));
        expect(button('Undo')).toBeDisabled();
        await user.click(button('Redo'));
        await waitFor(() => expect(value(BLACK_RED)).toBe(0.05));
        expect(button('Redo')).toBeDisabled();
      });

      it('leaves no step when the reset puts the value back to where it was', async () => {
        // A recipe that has the whole tone block, as the backend sends it.
        state.recipes['summer-f00001'] = makeRecipe('saved', {
          tone: {
            exposure_ev: 0,
            contrast: 0,
            highlights: 0,
            shadows: 0,
            whites: 0,
            blacks: 0,
            dehaze: 0,
          },
        });
        const { user } = await openFrame();
        await moveThenReset(user, 'Contrast');
        await waitFor(() => expect(value('Contrast')).toBe(0));
        expect(button('Undo')).toBeDisabled();
        expect(button('Redo')).toBeDisabled();
      });

      it('is a separate step when the reset comes more than half a second later', async () => {
        const { user } = await openFrame();
        const start = Date.now();
        let later = 0;
        vi.spyOn(Date, 'now').mockImplementation(() => start + later);
        slider(BLACK_RED).focus();
        await user.keyboard('{PageUp}');
        await waitFor(() => expect(button('Undo')).toBeEnabled());
        later = MERGE_WINDOW_MS + 1;
        fireEvent.doubleClick(slider(BLACK_RED));
        await waitFor(() => expect(value(BLACK_RED)).toBe(0.05));
        await user.click(button('Undo'));
        await waitFor(() => expect(value(BLACK_RED)).toBeGreaterThan(0.1));
        await user.click(button('Undo'));
        await waitFor(() => expect(value(BLACK_RED)).toBe(0.1));
        expect(button('Undo')).toBeDisabled();
      });

      it('is a separate step after a change to another slider', async () => {
        const { user } = await openFrame();
        await nudgeContrast(user);
        fireEvent.doubleClick(slider(BLACK_RED));
        await waitFor(() => expect(value(BLACK_RED)).toBe(0.05));
        await user.click(button('Undo'));
        await waitFor(() => expect(value(BLACK_RED)).toBe(0.1));
        expect(value('Contrast')).toBe(10);
        await user.click(button('Undo'));
        await waitFor(() => expect(value('Contrast')).toBe(0));
      });
    });

    it('does nothing while the automatic recipe is not known', async () => {
      const { user } = await openFrame({ analysis: false });
      fireEvent.doubleClick(slider(BLACK_RED));
      slider(BLACK_RED).focus();
      await user.keyboard('{Delete}');
      expect(value(BLACK_RED)).toBe(0.1);
      expect(button('Undo')).toBeDisabled();
    });
  });

  describe('Reset panel', () => {
    it('puts the Film base panel back to the automatic base and levels, as one step', async () => {
      const { user } = await openFrame();
      slider(BLACK_RED).focus();
      await user.keyboard('{ArrowRight}');
      await waitFor(() => expect(value(BLACK_RED)).toBeCloseTo(0.105));
      await user.click(button('Reset Film base to automatic'));
      await waitFor(() => expect(value(BLACK_RED)).toBe(0.05));
      expect(levelsOf(lastRender())).toEqual({
        black: [0.05, 0.06, 0.07],
        white: [0.95, 0.85, 0.75],
      });
      await user.click(button('Undo'));
      await waitFor(() => expect(value(BLACK_RED)).toBeCloseTo(0.105));
    });

    it('puts the Tone panel back, sliders and curves, and leaves the other panels alone', async () => {
      const { user } = await openFrame();
      await nudgeContrast(user);
      slider(BLACK_RED).focus();
      await user.keyboard('{ArrowRight}');
      await waitFor(() => expect(value(BLACK_RED)).toBeCloseTo(0.105));
      await user.click(button('Reset Tone to automatic'));
      await waitFor(() => expect(value('Contrast')).toBe(0));
      expect(value(BLACK_RED)).toBeCloseTo(0.105);
      await user.click(button('Undo'));
      await waitFor(() => expect(value('Contrast')).toBe(10));
    });

    it('has a button in the header of every panel', async () => {
      await openFrame();
      for (const panel of [
        'Crop, turn and straighten',
        'Film base',
        'Colour',
        'Tone',
        'Output sharpening',
      ]) {
        expect(button(`Reset ${panel} to automatic`)).toBeEnabled();
      }
    });

    it('puts the Colour and Sharpen panels back too', async () => {
      const { user } = await openFrame();
      await user.keyboard('q');
      await waitFor(() => expect(value('Cyan / red')).toBe(10));
      await user.click(button('Reset Colour to automatic'));
      await waitFor(() => expect(value('Cyan / red')).toBe(0));
      slider('Amount').focus();
      await user.keyboard('{PageUp}');
      await waitFor(() => expect(value('Amount')).toBeGreaterThan(0));
      await user.click(button('Reset Output sharpening to automatic'));
      await waitFor(() => expect(value('Amount')).toBe(0));
    });

    it('puts the whole geometry back to the automatic one: turns, flips and angle', async () => {
      const { user } = await openFrame();
      // The saved and the automatic recipe are turned once, flipped, and at 1.5 degrees.
      await user.click(button('Rotate right'));
      await user.click(button('Flip horizontally'));
      slider('Straighten').focus();
      await user.keyboard('{PageUp}');
      await waitFor(() =>
        expect(lastRender()).toMatchObject({ geometry: { rot90: 2, flip_h: false } }),
      );
      expect(value('Straighten')).not.toBe(1.5);
      await user.click(button('Reset Crop, turn and straighten to automatic'));
      await waitFor(() =>
        expect(lastRender()).toMatchObject({
          geometry: { rot90: 1, flip_h: true, angle_deg: 1.5 },
        }),
      );
      expect(value('Straighten')).toBe(1.5);
    });

    it('is the only reset of the Tone, Sharpen and Crop panels', async () => {
      await openFrame();
      for (const old of [
        'Reset tone',
        'Reset sharpening',
        'Reset dust removal',
        'Reset turns and straighten',
      ]) {
        expect(screen.queryByRole('button', { name: old })).toBeNull();
      }
      expect(screen.getAllByRole('button', { name: /^Reset .* to automatic$/ }).length).toBe(5);
    });

    it('is off while the automatic recipe is not known', async () => {
      await openFrame({ analysis: false });
      for (const panel of ['Film base', 'Colour', 'Tone', 'Output sharpening']) {
        expect(button(`Reset ${panel} to automatic`)).toBeDisabled();
      }
    });
  });

  describe('Reset all', () => {
    it('puts the whole recipe back to the automatic one, as one step', async () => {
      const { user } = await openFrame();
      await nudgeContrast(user);
      await user.click(button('Reset all'));
      await waitFor(() => expect(value('Contrast')).toBe(0));
      expect(lastRender()).toMatchObject({ tag: 'auto' });
      await waitFor(() => expect(savedRecipe()).toMatchObject({ tag: 'auto' }), {
        timeout: SAVE_DELAY_MS * 4,
      });
      await user.click(button('Undo'));
      await waitFor(() => expect(value('Contrast')).toBe(10));
      expect(lastRender()).toMatchObject({ tag: 'saved' });
      await user.click(button('Undo'));
      await waitFor(() => expect(value('Contrast')).toBe(0));
      expect(button('Undo')).toBeDisabled();
    });

    it('is off while the automatic recipe is not known', async () => {
      await openFrame({ analysis: false });
      expect(button('Reset all')).toBeDisabled();
    });
  });

  it('has no accessibility violations with the new buttons', async () => {
    const { container } = await openFrame();
    await expectNoA11yViolations(container);
  });
});
