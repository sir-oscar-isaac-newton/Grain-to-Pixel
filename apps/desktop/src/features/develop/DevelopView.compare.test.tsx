import { act, render, screen, waitFor, within } from '@testing-library/react';
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
  (await within(strip()).findAllByRole('option')).find(
    (o) => o.getAttribute('aria-label') === `Frame ${n}`,
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
