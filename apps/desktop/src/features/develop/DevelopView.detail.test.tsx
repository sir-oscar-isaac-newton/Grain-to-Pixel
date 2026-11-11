import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
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

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

const F1 = 'summer-f00001';
const F2 = 'summer-f00002';
const F3 = 'summer-f00003';

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

const button = (name: string) => screen.getByRole('button', { name });
const zoomText = () => screen.getByLabelText('Zoom level').textContent;
const NOTICE = 'Full size, with output sharpening as in a full-size export.';
const RENDERING = 'Rendering detail…';
/** The full-size layer, once it is on screen. */
const layerNow = () => screen.queryByRole('img', { name: 'Frame 1 at full size' });
const layer = () => waitFor(() => expect(layerNow()).toBeInTheDocument());
const calls = () => ipc.developRenderDetail.mock.calls;
const recipeTag = (recipe: unknown) => (recipe as { tag?: string }).tag;

describe('Develop: full-size detail', () => {
  let restoreLayout: () => void;

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    // A stage of 900 x 600; the full-size picture is 4096 x 2731 by default, so 1:1 is 200%.
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

  async function openFrame() {
    const user = userEvent.setup();
    const view = render(<DevelopView />);
    await waitFor(() => expect(screen.getByRole('img', { name: 'Frame 1' })).toBeInTheDocument());
    return { user, ...view };
  }

  /** Opens the frame and goes to 1:1, with the full-size layer on screen. */
  async function atOneToOne() {
    const opened = await openFrame();
    await opened.user.click(button('1:1'));
    await layer();
    return opened;
  }

  /** A change of the recipe that is one step (the key `q` moves the red-cyan slider). */
  const tap = () => {
    fireEvent.keyDown(window, { key: 'q', repeat: false });
    fireEvent.keyUp(window, { key: 'q' });
  };

  const pan = (stage: Element, dx: number, dy = 0) => {
    fireEvent.mouseDown(stage, { button: 0, clientX: 400, clientY: 300 });
    fireEvent.mouseMove(window, { clientX: 400 + dx, clientY: 300 + dy });
    fireEvent.mouseUp(window);
  };

  describe('when it is asked for', () => {
    it('asks for nothing while the zoom is below 1:1, and one tiny region to learn the size past 100%', async () => {
      const { user } = await openFrame();
      expect(calls()).toHaveLength(0);
      await user.click(button('Zoom in'));
      await user.click(button('Zoom in'));
      expect(Number.parseFloat(zoomText() ?? '0')).toBeLessThan(100);
      expect(calls()).toHaveLength(0);

      await user.click(button('Zoom in'));
      await user.click(button('Zoom in'));
      expect(Number.parseFloat(zoomText() ?? '0')).toBeGreaterThan(100);
      await waitFor(() => expect(calls()).toHaveLength(1));
      const [id, , region] = calls()[0] ?? [];
      expect(id).toBe(F1);
      expect(region).toEqual({ x: 0.495, y: 0.495, w: 0.01, h: 0.01 });
      // Learning the size is not showing the picture: that waits for 1:1.
      expect(layerNow()).not.toBeInTheDocument();
      expect(screen.queryByText(NOTICE)).not.toBeInTheDocument();
    });

    it('goes to 1:1 with the button, shows the readout, and asks for the visible part plus a margin', async () => {
      await atOneToOne();
      expect(zoomText()).toBe('1:1');
      expect(button('1:1')).toBeInTheDocument();
      // The learning request, then the part around the centre: 900 x 600 pixels visible, grown by
      // half of that on each side, of a picture 4096 x 2731.
      expect(calls()).toHaveLength(2);
      const [id, recipe, region, seq] = calls()[1] ?? [];
      expect(id).toBe(F1);
      expect(recipeTag(recipe)).toBe('saved');
      expect(seq).toBeGreaterThan(calls()[0]?.[3] as number);
      expect(region?.w).toBeCloseTo(1800 / 4096, 3);
      // (The fake preview is a hair wider than the picture, so the height is only close.)
      expect(region?.h).toBeCloseTo(1200 / 2731, 2);
      expect((region?.x ?? 0) + (region?.w ?? 0) / 2).toBeCloseTo(0.5, 3);
      // One pixel of the layer is one screen pixel at 1:1.
      const shown = layerNow() as HTMLElement;
      expect(Number.parseFloat(shown.style.width)).toBeGreaterThanOrEqual(1800);
      expect(Number.parseFloat(shown.style.width)).toBeLessThan(1803);
      expect(shown.className).not.toMatch(/enlarged/);
      expect(screen.getByText(NOTICE)).toBeInTheDocument();
    });

    it('lets the zoom reach 200% of the scan once its size is known', async () => {
      state.detailFull = { width: 8192, height: 5461 };
      const { user } = await openFrame();
      await user.click(button('1:1'));
      await layer();
      expect(zoomText()).toBe('1:1');
      for (let i = 0; i < 14; i += 1) await user.click(button('Zoom in'));
      expect(zoomText()).toBe('800%');
      // Enlarged past 1:1: the pixels are not smoothed.
      expect((layerNow() as HTMLElement).className).toMatch(/enlarged/);
    });

    it('goes back to the proxy when zooming out, and the notice goes with the layer', async () => {
      const { user } = await atOneToOne();
      expect(screen.getByText(NOTICE)).toBeInTheDocument();
      await user.click(button('Zoom out'));
      expect(layerNow()).not.toBeInTheDocument();
      expect(screen.queryByText(NOTICE)).not.toBeInTheDocument();
      expect(screen.queryByText(RENDERING)).not.toBeInTheDocument();
      await user.click(button('1:1'));
      await layer();
      await user.click(button('Fit'));
      expect(layerNow()).not.toBeInTheDocument();
      expect(screen.queryByText(NOTICE)).not.toBeInTheDocument();
    });
  });

  describe('changes of the recipe', () => {
    it('asks for nothing while a key is held, hides the layer, and asks once it is let go', async () => {
      await atOneToOne();
      const before = calls().length;
      let release = () => {};
      state.detailGate = new Promise<void>((resolve) => {
        release = resolve;
      });
      // The first press is a step like any other; the repeats that follow are a drag.
      fireEvent.keyDown(window, { key: 'q', repeat: false });
      await waitFor(() => expect(calls()).toHaveLength(before + 1));
      fireEvent.keyDown(window, { key: 'q', repeat: true });
      fireEvent.keyDown(window, { key: 'q', repeat: true });
      // The enlarged preview shows meanwhile.
      expect(await screen.findByText(RENDERING)).toBeInTheDocument();
      expect(layerNow()).not.toBeInTheDocument();
      expect(screen.queryByText(NOTICE)).not.toBeInTheDocument();
      await act(async () => {});
      expect(calls()).toHaveLength(before + 1);

      fireEvent.keyUp(window, { key: 'q' });
      await waitFor(() => expect(calls()).toHaveLength(before + 2));
      // Still waiting for it.
      expect(screen.getByText(RENDERING)).toBeInTheDocument();
      expect(layerNow()).not.toBeInTheDocument();
      await act(async () => {
        release();
        await Promise.resolve();
      });
      await layer();
      expect(screen.queryByText(RENDERING)).not.toBeInTheDocument();
      expect(screen.getByText(NOTICE)).toBeInTheDocument();
      expect(calls()).toHaveLength(before + 2);
      // What was drawn is the last request's: the red-cyan value the key moved it to.
      expect(calls().at(-1)?.[1]).toMatchObject({ color: { red_cyan: 30 } });
    });

    it('asks again for a recipe that changed in one step, and hides the old picture until then', async () => {
      await atOneToOne();
      const before = calls().length;
      let release = () => {};
      state.detailGate = new Promise<void>((resolve) => {
        release = resolve;
      });
      tap();
      await waitFor(() => expect(calls()).toHaveLength(before + 1));
      expect(layerNow()).not.toBeInTheDocument();
      expect(screen.getByText(RENDERING)).toBeInTheDocument();
      await act(async () => {
        release();
        await Promise.resolve();
      });
      await layer();
    });
  });

  describe('panning', () => {
    it('makes no request inside the region on hand, and one when the view leaves it', async () => {
      const { container } = await atOneToOne();
      const stage = container.querySelector('[aria-busy]') as HTMLElement;
      const before = calls().length;
      // 450 pixels of margin on each side: 100 is inside it.
      pan(stage, -100);
      await act(async () => {});
      expect(calls()).toHaveLength(before);
      expect(layerNow()).toBeInTheDocument();
      // Another 500 leaves it.
      pan(stage, -500);
      await waitFor(() => expect(calls()).toHaveLength(before + 1));
      const region = calls()[before]?.[2];
      // To the right of the first region now: the picture moved left under the view.
      expect(region?.x).toBeGreaterThan(calls()[before - 1]?.[2]?.x ?? 1);
      await layer();
    });
  });

  describe('when it goes wrong', () => {
    it('does not show a request a newer one replaced as an error', async () => {
      await atOneToOne();
      state.detailCancel = true;
      tap();
      await waitFor(() => expect(ipc.developRenderDetail.mock.results.length).toBeGreaterThan(2));
      await act(async () => {});
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    });

    it('says when the detail could not be made, and keeps the preview', async () => {
      await atOneToOne();
      state.detailFail = 'The scan could not be read.';
      tap();
      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent(
        'The full-size detail could not be drawn: The scan could not be read.',
      );
      expect(screen.getByRole('img', { name: 'Frame 1' })).toBeInTheDocument();
      expect(layerNow()).not.toBeInTheDocument();
      expect(screen.queryByText(RENDERING)).not.toBeInTheDocument();

      // The next change tries again, and a success clears the message.
      state.detailFail = null;
      tap();
      await layer();
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    });
  });

  describe('with the other parts of the view', () => {
    it('asks for the automatic conversion while "before" is on, and for the edit again after', async () => {
      const { user } = await atOneToOne();
      await user.click(button('Before'));
      await waitFor(() => expect(recipeTag(calls().at(-1)?.[1])).toBe('auto'));
      await layer();
      await user.click(button('Before'));
      await waitFor(() => expect(recipeTag(calls().at(-1)?.[1])).toBe('saved'));
    });

    /** Zooms in with the wheel, which is quicker than the button. */
    const zoomInALot = (container: HTMLElement) => {
      const stage = container.querySelector('[aria-busy]') as HTMLElement;
      for (let i = 0; i < 12; i += 1) {
        fireEvent.wheel(stage, { deltaY: -100, clientX: 450, clientY: 300 });
      }
    };

    it('is off while the crop is edited, which works on the preview', async () => {
      const { user, container } = await atOneToOne();
      await user.click(button('Edit crop'));
      expect(layerNow()).not.toBeInTheDocument();
      expect(button('1:1')).toBeDisabled();
      const before = calls().length;
      zoomInALot(container);
      await act(async () => {});
      expect(calls()).toHaveLength(before);
      expect(layerNow()).not.toBeInTheDocument();
      expect(screen.queryByText(RENDERING)).not.toBeInTheDocument();
    });

    it('is off while a picker is active, which works on the preview', async () => {
      const { user, container } = await atOneToOne();
      await user.click(button('Pick film base'));
      expect(layerNow()).not.toBeInTheDocument();
      expect(button('1:1')).toBeDisabled();
      const before = calls().length;
      zoomInALot(container);
      await act(async () => {});
      expect(calls()).toHaveLength(before);
      expect(layerNow()).not.toBeInTheDocument();
      expect(screen.queryByText(RENDERING)).not.toBeInTheDocument();
    });

    it('asks only for the current frame while comparing, and the reference stays a preview', async () => {
      const { user } = await openFrame();
      await user.click(button('Compare with'));
      const strip = screen.getByRole('listbox', { name: 'Frames of the current Library view' });
      const options = await screen.findAllByRole('option');
      expect(strip).toBeInTheDocument();
      await user.click(
        options.find((o) =>
          /^Frame 3(,|$)/.test(o.getAttribute('aria-label') ?? ''),
        ) as HTMLElement,
      );
      await screen.findByRole('img', { name: 'Reference, Frame 3' });
      await user.click(button('1:1'));
      await layer();
      expect(calls().length).toBeGreaterThan(1);
      expect(calls().every((c) => c[0] === F1)).toBe(true);
      expect(
        screen.queryByRole('img', { name: /Reference, Frame 3 at full size/ }),
      ).not.toBeInTheDocument();
    });
  });

  describe('another frame', () => {
    it('ignores a late answer for the frame that was left', async () => {
      const { user } = await openFrame();
      let release = () => {};
      state.detailGate = new Promise<void>((resolve) => {
        release = resolve;
      });
      await user.click(button('1:1'));
      await waitFor(() => expect(calls()).toHaveLength(1));
      // The frame changes while the size is still being learned.
      act(() => useUiStore.setState({ developFrame: { id: F2, index: 1 } }));
      await waitFor(() => expect(screen.getByRole('img', { name: 'Frame 2' })).toBeInTheDocument());
      state.detailGate = null;
      await act(async () => {
        release();
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(screen.queryByRole('img', { name: /at full size/ })).not.toBeInTheDocument();
      expect(zoomText()).not.toBe('1:1');
      expect(calls().every((c) => c[0] === F1)).toBe(true);
      expect(screen.queryByText(NOTICE)).not.toBeInTheDocument();
    });
  });

  describe('accessibility', () => {
    it('has no violations with the layer and its notice on screen', async () => {
      const { container } = await atOneToOne();
      expect(screen.getByText(NOTICE)).toBeInTheDocument();
      await expectNoA11yViolations(container);
    });
  });
});
