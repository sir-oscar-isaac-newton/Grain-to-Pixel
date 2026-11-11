import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../app/store';
import { expectNoA11yViolations, mockLayout } from '../../test-support';
import {
  ipc,
  libraryDetails,
  makeFrames,
  makePreview,
  makeRecipe,
  makeRoll,
  resetFake,
  state,
} from '../library/fake-ipc';
import { resetLibraryStore, useLibraryStore } from '../library/library-store';
import { resetThumbnails } from '../library/thumbnails';
import { DevelopView } from './DevelopView';
import { SAVE_DELAY_MS } from './use-develop';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

const F1 = 'summer-f00001';
const F3 = 'summer-f00003';
const F4 = 'summer-f00004';

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

const frameNow = () => useUiStore.getState().developFrame;
const button = (name: string) => screen.getByRole('button', { name });
const strip = () => screen.getByRole('listbox', { name: 'Frames of the current Library view' });
const stripFrame = async (n: number) =>
  (await within(strip()).findAllByRole('option')).find((o) =>
    new RegExp(`^Frame ${n}(,|$)`).test(o.getAttribute('aria-label') ?? ''),
  ) as HTMLElement;
const picture = (name: string) =>
  waitFor(() => {
    const found = [...document.querySelectorAll('canvas[role="img"]')].find(
      (c) => c.getAttribute('aria-label') === name,
    );
    if (!found) throw new Error(`no picture named ${name}`);
    return found;
  });
const rendersOf = (id: string) => ipc.developRender.mock.calls.filter((c) => c[0] === id);
/** The two stages: the current frame's first, the reference's second. */
const stages = (container: HTMLElement) =>
  [...container.querySelectorAll('[aria-busy]')] as HTMLElement[];
const widthOf = (picture: Element) => Number.parseFloat((picture as HTMLElement).style.width);
const shiftOf = (picture: Element) => {
  const match = /translate\(([-\d.e]+)px, ([-\d.e]+)px\)$/.exec(
    (picture as HTMLElement).style.transform,
  );
  return { x: Number(match?.[1]), y: Number(match?.[2]) };
};
const savedFor = (id: string) => state.saved.filter((s) => s.id === id);

describe('Develop: compare', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    restoreLayout = mockLayout(900, 600);
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => null);
    state.recipes[F3] = makeRecipe('ref');
    openLibrary();
    useUiStore.setState({ view: 'develop', developFrame: { id: F1, index: 0 } });
  });
  afterEach(() => {
    restoreLayout();
    vi.restoreAllMocks();
  });

  /** Opens frame 1, arms compare mode and chooses frame 3 as the reference. */
  async function compareWithThree() {
    const user = userEvent.setup();
    const view = render(<DevelopView />);
    await picture('Frame 1');
    await user.click(button('Compare with'));
    await user.click(await stripFrame(3));
    await picture('Reference, Frame 3');
    return { user, ...view };
  }

  /** The contrast slider moved one page up with the keyboard: an edit of the current frame. */
  async function editCurrent(user: ReturnType<typeof userEvent.setup>) {
    const slider = screen.getByRole('slider', { name: 'Contrast' });
    slider.focus();
    await user.keyboard('{PageUp}');
    await waitFor(() => expect(slider.getAttribute('aria-valuenow')).toBe('10'));
  }

  describe('entering', () => {
    it('waits for a click on the filmstrip, which chooses the reference and keeps the frame open', async () => {
      const user = userEvent.setup();
      render(<DevelopView />);
      await picture('Frame 1');
      const toggle = button('Compare with');
      expect(toggle).toHaveAttribute('aria-pressed', 'false');
      await user.click(toggle);
      expect(toggle).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByText(/Choose the frame to compare with/)).toBeInTheDocument();
      expect(screen.queryByRole('img', { name: /^Reference/ })).not.toBeInTheDocument();

      await user.click(await stripFrame(3));
      await picture('Reference, Frame 3');
      expect(frameNow()).toEqual({ id: F1, index: 0 });
      expect(screen.queryByText(/Choose the frame to compare with/)).not.toBeInTheDocument();
      expect(screen.getByText('Current: Frame 1')).toBeInTheDocument();
      expect(screen.getByText('Reference: Frame 3')).toBeInTheDocument();
      await picture('Frame 1');
    });

    it('draws the reference with its own saved recipe, fitted, and saves nothing for it', async () => {
      await compareWithThree();
      expect(ipc.developOpen).toHaveBeenCalledWith(F3);
      const [, recipe, edge] = rendersOf(F3)[0] ?? [];
      expect(recipe).toMatchObject({ tag: 'ref' });
      // The pane is 900 x 600: its picture is asked for at that long edge, in steps of 128.
      expect(edge).toBe(1024);
      const reference = await picture('Reference, Frame 3');
      expect(reference).toHaveStyle({ transform: 'translate(-50%, -50%) translate(0px, 0px)' });
      await new Promise((resolve) => setTimeout(resolve, SAVE_DELAY_MS + 100));
      expect(savedFor(F3)).toEqual([]);
    });

    it('draws a reference that has no saved recipe as Develop opens such a frame', async () => {
      // The backend gives the starting recipe for a frame without a saved one; the fake's "auto".
      state.recipes[F3] = makeRecipe('auto');
      await compareWithThree();
      expect(rendersOf(F3)[0]?.[1]).toMatchObject({ tag: 'auto' });
      expect(savedFor(F3)).toEqual([]);
    });

    it('keeps the reference while the filmstrip opens other frames as usual', async () => {
      const { user } = await compareWithThree();
      await user.click(await stripFrame(4));
      await waitFor(() => expect(frameNow()).toEqual({ id: F4, index: 3 }));
      await picture('Frame 4');
      expect(screen.getByText('Reference: Frame 3')).toBeInTheDocument();
      expect(rendersOf(F3)).toHaveLength(1);
    });

    it('drops the reference, and asks for another, when the frame itself is opened', async () => {
      const { user } = await compareWithThree();
      await user.click(await stripFrame(3));
      await waitFor(() => expect(frameNow()).toEqual({ id: F3, index: 2 }));
      await picture('Frame 3');
      expect(screen.queryByRole('img', { name: /^Reference/ })).not.toBeInTheDocument();
      expect(button('Compare with')).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByText(/Choose the frame to compare with/)).toBeInTheDocument();
    });

    it('tells when the reference could not be drawn, and leaves the current frame alone', async () => {
      state.renderFails[F3] = 'Out of memory';
      const user = userEvent.setup();
      render(<DevelopView />);
      await picture('Frame 1');
      await user.click(button('Compare with'));
      await user.click(await stripFrame(3));
      expect(
        await screen.findByText(/The reference frame could not be shown: Out of memory/),
      ).toBeInTheDocument();
      expect(screen.queryByRole('img', { name: /^Reference/ })).not.toBeInTheDocument();
      expect(screen.getByRole('img', { name: 'Frame 1' })).toBeInTheDocument();
      expect(screen.getByRole('slider', { name: 'Contrast' })).toBeEnabled();
    });

    it('says when the reference frame could not be opened', async () => {
      state.noScan.add(F3);
      const user = userEvent.setup();
      render(<DevelopView />);
      await picture('Frame 1');
      await user.click(button('Compare with'));
      await user.click(await stripFrame(3));
      expect(
        await screen.findByText(
          /The reference frame could not be shown: This frame has no scan file/,
        ),
      ).toBeInTheDocument();
    });
  });

  describe('editing', () => {
    it('changes only the current frame: its renders and its saves', async () => {
      const { user } = await compareWithThree();
      const referenceRenders = rendersOf(F3).length;
      await editCurrent(user);
      await waitFor(() => expect(savedFor(F1)).toHaveLength(1), { timeout: SAVE_DELAY_MS * 4 });
      expect(savedFor(F1)[0]?.recipe).toMatchObject({ tone: { contrast: 10 } });
      expect(savedFor(F3)).toEqual([]);
      expect(rendersOf(F1).at(-1)?.[1]).toMatchObject({ tone: { contrast: 10 } });
      expect(rendersOf(F3)).toHaveLength(referenceRenders);
      expect(ipc.developOpen.mock.calls.filter((c) => c[0] === F3)).toHaveLength(1);
    });

    it('keeps undo, redo, reset and Before for the current frame', async () => {
      const { user } = await compareWithThree();
      await editCurrent(user);
      await user.click(button('Undo'));
      await waitFor(() =>
        expect(screen.getByRole('slider', { name: 'Contrast' }).getAttribute('aria-valuenow')).toBe(
          '0',
        ),
      );
      await user.click(button('Redo'));
      await waitFor(() =>
        expect(screen.getByRole('slider', { name: 'Contrast' }).getAttribute('aria-valuenow')).toBe(
          '10',
        ),
      );
      await waitFor(() => expect(button('Reset all')).toBeEnabled());
      await user.click(button('Reset all'));
      await waitFor(() => expect(rendersOf(F1).at(-1)?.[1]).toMatchObject({ tag: 'auto' }));
      expect(button('Before')).toBeEnabled();
      expect(rendersOf(F3)).toHaveLength(1);
    });
  });

  describe('swapping', () => {
    it('saves the edit, then the reference becomes the frame and the frame the reference', async () => {
      const { user } = await compareWithThree();
      await editCurrent(user);
      await user.click(button('Swap frames'));
      await waitFor(() => expect(frameNow()).toEqual({ id: F3, index: 2 }));
      await picture('Frame 3');
      await picture('Reference, Frame 1');
      expect(screen.getByText('Current: Frame 3')).toBeInTheDocument();
      expect(screen.getByText('Reference: Frame 1')).toBeInTheDocument();
      // The edit of frame 1 reached the backend before it was opened again as the reference.
      expect(savedFor(F1)).toHaveLength(1);
      const saveOrder = ipc.recipeSave.mock.invocationCallOrder[0] ?? 0;
      const openOrders = ipc.developOpen.mock.calls
        .map((c, i) => [c[0], ipc.developOpen.mock.invocationCallOrder[i] ?? 0] as const)
        .filter(([id]) => id === F1)
        .map(([, order]) => order);
      expect(openOrders.at(-1)).toBeGreaterThan(saveOrder);
      expect(savedFor(F3)).toEqual([]);
    });

    it('stays as it is when the edit cannot be saved', async () => {
      const { user } = await compareWithThree();
      await editCurrent(user);
      state.saveFails = 'The disk is full.';
      await user.click(button('Swap frames'));
      expect(
        await screen.findByText(
          /The edit of this frame could not be saved, so the frames were not swapped/,
        ),
      ).toBeInTheDocument();
      expect(frameNow()).toEqual({ id: F1, index: 0 });
      expect(screen.getByText('Reference: Frame 3')).toBeInTheDocument();
    });

    it('is not offered until a reference is chosen', async () => {
      const user = userEvent.setup();
      render(<DevelopView />);
      await picture('Frame 1');
      expect(screen.queryByRole('button', { name: 'Swap frames' })).not.toBeInTheDocument();
      await user.click(button('Compare with'));
      expect(button('Swap frames')).toBeDisabled();
    });
  });

  describe('linked zoom and pan', () => {
    const zoomIn = async (user: ReturnType<typeof userEvent.setup>, times: number) => {
      for (let i = 0; i < times; i += 1) await user.click(button('Zoom in'));
    };

    it('has a switch that is on by default and is kept when the reference or the frame changes', async () => {
      const { user } = await compareWithThree();
      const link = button('Link zoom');
      expect(link).toHaveAttribute('aria-pressed', 'true');
      await user.click(link);
      expect(link).toHaveAttribute('aria-pressed', 'false');
      await user.click(await stripFrame(4));
      await waitFor(() => expect(frameNow()).toEqual({ id: F4, index: 3 }));
      await picture('Frame 4');
      expect(button('Link zoom')).toHaveAttribute('aria-pressed', 'false');
    });

    it('is shown while compare mode is on, and not outside it', async () => {
      const user = userEvent.setup();
      render(<DevelopView />);
      await picture('Frame 1');
      expect(screen.queryByRole('button', { name: 'Link zoom' })).not.toBeInTheDocument();
      await user.click(button('Compare with'));
      expect(button('Link zoom')).toBeInTheDocument();
      await user.click(button('Leave compare'));
      expect(screen.queryByRole('button', { name: 'Link zoom' })).not.toBeInTheDocument();
    });

    it('zooms the reference with the current frame, and fits it again with Fit', async () => {
      const { user, container } = await compareWithThree();
      const reference = await picture('Reference, Frame 3');
      const fitted = widthOf(reference);
      await zoomIn(user, 3);
      const current = await picture('Frame 1');
      await waitFor(() => expect(widthOf(reference)).toBeGreaterThan(fitted * 1.9));
      expect(widthOf(reference)).toBeCloseTo(widthOf(current), 6);
      expect(stages(container)).toHaveLength(2);
      await user.click(button('Fit'));
      await waitFor(() => expect(widthOf(reference)).toBeCloseTo(fitted, 6));
    });

    it('moves the reference with the current frame when it is panned, without drawing it again', async () => {
      const { user, container } = await compareWithThree();
      await zoomIn(user, 3);
      // 0.4395 x 1.25^3 of 2048 px is 1758 px.
      await waitFor(() => expect(rendersOf(F3).at(-1)?.[2]).toBe(1792));
      const reference = await picture('Reference, Frame 3');
      const renders = rendersOf(F3).length;
      const opens = ipc.developOpen.mock.calls.length;
      const [stage] = stages(container);
      fireEvent.mouseDown(stage as HTMLElement, { button: 0, clientX: 300, clientY: 300 });
      fireEvent.mouseMove(window, { clientX: 200, clientY: 250 });
      fireEvent.mouseUp(window);
      await waitFor(() => expect(shiftOf(reference).x).toBeCloseTo(-100, 4));
      // The fake's two preview sizes round to slightly different heights.
      expect(shiftOf(reference).y).toBeCloseTo(-50, 0);
      expect(shiftOf(await picture('Frame 1'))).toEqual({ x: -100, y: -50 });
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(rendersOf(F3)).toHaveLength(renders);
      expect(ipc.developOpen.mock.calls).toHaveLength(opens);
    });

    it('takes no pointer or wheel input of its own', async () => {
      const { user, container } = await compareWithThree();
      await zoomIn(user, 4);
      const reference = await picture('Reference, Frame 3');
      const before = { width: widthOf(reference), shift: shiftOf(reference) };
      const [, stage] = stages(container);
      fireEvent.wheel(stage as HTMLElement, { deltaY: -100, clientX: 450, clientY: 300 });
      fireEvent.mouseDown(stage as HTMLElement, { button: 0, clientX: 300, clientY: 300 });
      fireEvent.mouseMove(window, { clientX: 200, clientY: 250 });
      fireEvent.mouseUp(window);
      expect(widthOf(reference)).toBe(before.width);
      expect(shiftOf(reference)).toEqual(before.shift);
    });

    it('keeps the reference fitted while the switch is off', async () => {
      const { user } = await compareWithThree();
      await user.click(button('Link zoom'));
      const reference = await picture('Reference, Frame 3');
      const fitted = widthOf(reference);
      const renders = rendersOf(F3).length;
      await zoomIn(user, 4);
      expect(widthOf(await picture('Frame 1'))).toBeGreaterThan(fitted * 2);
      expect(widthOf(reference)).toBe(fitted);
      expect(shiftOf(reference)).toEqual({ x: 0, y: 0 });
      expect(rendersOf(F3)).toHaveLength(renders);
      // Switched on again: the reference follows at once.
      await user.click(button('Link zoom'));
      await waitFor(() => expect(widthOf(reference)).toBeGreaterThan(fitted * 2));
    });

    it('asks for the long edge the zoom needs, in steps, and only when the step changes', async () => {
      const { user } = await compareWithThree();
      expect(rendersOf(F3).at(-1)?.[2]).toBe(1024);
      // One step in: 0.4395 x 1.25 of 2048 px is 1125 px, so 1152.
      await user.click(button('Zoom in'));
      await waitFor(() => expect(rendersOf(F3).at(-1)?.[2]).toBe(1152));
      const renders = rendersOf(F3).length;
      // Back to Fit (the pane's own edge, 1024).
      await user.click(button('Fit'));
      await waitFor(() => expect(rendersOf(F3).at(-1)?.[2]).toBe(1024));
      expect(rendersOf(F3).length).toBe(renders + 1);
      // The most zoomed: capped at the release edge.
      for (let i = 0; i < 12; i += 1) await user.click(button('Zoom in'));
      await waitFor(() => expect(rendersOf(F3).at(-1)?.[2]).toBe(2048));
    });

    it('asks for the pane size, as before, while the switch is off', async () => {
      const { user } = await compareWithThree();
      await user.click(button('Link zoom'));
      await zoomIn(user, 4);
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(new Set(rendersOf(F3).map((c) => c[2]))).toEqual(new Set([1024]));
    });
  });

  describe('keeping the reference current', () => {
    /** The fake saves nothing to read back; this one does, as the backend does. */
    function saveToRecipes() {
      ipc.recipeSave.mockImplementation(async (id, recipe) => {
        state.saved.push({ id, recipe });
        state.recipes[id] = recipe;
        return recipe;
      });
    }

    it('reads the saved recipe again after each swap', async () => {
      saveToRecipes();
      const { user } = await compareWithThree();
      await editCurrent(user);
      await user.click(button('Swap frames'));
      await waitFor(() => expect(frameNow()).toEqual({ id: F3, index: 2 }));
      await picture('Reference, Frame 1');
      await waitFor(() =>
        expect(rendersOf(F1).at(-1)?.[1]).toMatchObject({ tone: { contrast: 10 } }),
      );

      // Swapped back: frame 3 is read again too, and still has its own recipe.
      await user.click(button('Swap frames'));
      await waitFor(() => expect(frameNow()).toEqual({ id: F1, index: 0 }));
      await picture('Reference, Frame 3');
      await waitFor(() =>
        expect(ipc.developOpen.mock.calls.filter((c) => c[0] === F3).length).toBeGreaterThan(1),
      );
      expect(rendersOf(F3).at(-1)?.[1]).toMatchObject({ tag: 'ref' });
    });

    it('draws the reference again, with the new recipe, after Apply settings changed it', async () => {
      const { user } = await compareWithThree();
      useLibraryStore.getState().setSelected([F1, F3]);
      const renders = rendersOf(F3).length;
      await user.click(button('Apply settings…'));
      const dialog = within(await screen.findByRole('dialog', { name: 'Apply settings' }));
      await dialog.findByRole('radio', { name: 'The other selected frame (1)' });
      // What the backend now has saved for the reference.
      state.recipes[F3] = makeRecipe('applied');
      await user.click(dialog.getByRole('button', { name: 'Apply colour and tone to 1 frame' }));
      await dialog.findByText('Applied to 1 frame.');
      await waitFor(() => expect(rendersOf(F3)).toHaveLength(renders + 1));
      expect(rendersOf(F3).at(-1)?.[1]).toMatchObject({ tag: 'applied' });
      expect(savedFor(F3)).toEqual([]);
    });
  });

  describe('the filmstrip', () => {
    it('names the reference frame, besides marking it', async () => {
      await compareWithThree();
      const options = await within(strip()).findAllByRole('option');
      const names = options.map((o) => o.getAttribute('aria-label'));
      expect(names).toContain('Frame 3, reference');
      expect(names).toContain('Frame 1');
      expect(names.filter((n) => n?.includes('reference'))).toHaveLength(1);
    });

    it('names no reference once compare mode is left', async () => {
      const { user } = await compareWithThree();
      await user.click(button('Leave compare'));
      const options = await within(strip()).findAllByRole('option');
      expect(options.map((o) => o.getAttribute('aria-label'))).toContain('Frame 3');
    });
  });

  describe('leaving', () => {
    it('closes the reference with the button, and the frame stays open', async () => {
      const { user } = await compareWithThree();
      await user.click(button('Leave compare'));
      expect(screen.queryByRole('img', { name: /^Reference/ })).not.toBeInTheDocument();
      expect(screen.queryByText('Reference: Frame 3')).not.toBeInTheDocument();
      expect(button('Compare with')).toHaveAttribute('aria-pressed', 'false');
      expect(screen.queryByRole('button', { name: 'Leave compare' })).not.toBeInTheDocument();
      expect(frameNow()).toEqual({ id: F1, index: 0 });
      expect(useUiStore.getState().view).toBe('develop');
    });

    it('closes the reference with Escape before Escape leaves the view', async () => {
      const { user } = await compareWithThree();
      await user.keyboard('{Escape}');
      expect(screen.queryByRole('img', { name: /^Reference/ })).not.toBeInTheDocument();
      expect(button('Compare with')).toHaveAttribute('aria-pressed', 'false');
      expect(useUiStore.getState().view).toBe('develop');
      await user.keyboard('{Escape}');
      expect(useUiStore.getState().view).toBe('library');
    });

    it('leaves compare mode with Escape while it is still waiting for a reference', async () => {
      const user = userEvent.setup();
      render(<DevelopView />);
      await picture('Frame 1');
      await user.click(button('Compare with'));
      await user.keyboard('{Escape}');
      expect(button('Compare with')).toHaveAttribute('aria-pressed', 'false');
      expect(useUiStore.getState().view).toBe('develop');
    });

    it('gives Escape to a dialog first, then to picking, before compare mode', async () => {
      const { user } = await compareWithThree();
      await user.click(button('Export…'));
      await screen.findByRole('dialog');
      await user.keyboard('{Escape}');
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(screen.getByText('Reference: Frame 3')).toBeInTheDocument();

      await user.click(screen.getByRole('button', { name: 'Pick white point' }));
      await user.keyboard('{Escape}');
      expect(screen.getByText('Reference: Frame 3')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Pick white point' })).toHaveAttribute(
        'aria-pressed',
        'false',
      );
      await user.keyboard('{Escape}');
      expect(screen.queryByText('Reference: Frame 3')).not.toBeInTheDocument();
    });

    it('draws nothing when the reference arrives after compare mode was left', async () => {
      let finish: (() => void) | undefined;
      ipc.developRender.mockImplementation(async (id, recipe, edge, seq) => {
        if (id === F3) await new Promise<void>((resolve) => (finish = resolve));
        return makePreview(recipe, edge, seq);
      });
      const user = userEvent.setup();
      render(<DevelopView />);
      await picture('Frame 1');
      await user.click(button('Compare with'));
      await user.click(await stripFrame(3));
      expect(await screen.findByText('Opening the reference frame…')).toBeInTheDocument();
      await user.click(button('Leave compare'));
      await act(async () => {
        finish?.();
      });
      expect(screen.queryByRole('img', { name: /^Reference/ })).not.toBeInTheDocument();
      expect(screen.getByRole('img', { name: 'Frame 1' })).toBeInTheDocument();
    });
  });

  it('draws the reference again when its pane changes size, and not otherwise', async () => {
    const callbacks: Array<() => void> = [];
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(callback: () => void) {
          callbacks.push(callback);
        }
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    await compareWithThree();
    const before = rendersOf(F3).length;
    // Told of a resize that left the size as it was: nothing is drawn again.
    act(() => callbacks.forEach((callback) => callback()));
    expect(rendersOf(F3)).toHaveLength(before);
    // The pane becomes 500 x 300: a picture of that size is asked for.
    restoreLayout();
    restoreLayout = mockLayout(500, 300);
    act(() => callbacks.forEach((callback) => callback()));
    await waitFor(() => expect(rendersOf(F3)).toHaveLength(before + 1));
    expect(rendersOf(F3).at(-1)?.[2]).toBe(512);
    vi.unstubAllGlobals();
  });

  it('has no accessibility violations while comparing', async () => {
    await compareWithThree();
    await expectNoA11yViolations(document.body);
  });
});
