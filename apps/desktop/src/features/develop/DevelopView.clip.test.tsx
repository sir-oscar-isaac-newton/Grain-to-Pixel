import { act, render, screen, waitFor, within } from '@testing-library/react';
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
import { CLIP_COLOURS } from './clip';
import { DevelopView } from './DevelopView';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

const F1 = 'summer-f00001';
const F2 = 'summer-f00002';

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

/** jsdom has no canvas: a context that records what each layer is given, and an `ImageData`. */
class FakeImageData {
  constructor(
    readonly data: Uint8ClampedArray,
    readonly width: number,
    readonly height: number,
  ) {}
}

const button = (name: string) => screen.getByRole('button', { name });
const layers = () => document.querySelectorAll('canvas[data-layer="clipping"]');
const HIGHLIGHTS = 'Red: highlights clipped.';
const SHADOWS = 'Blue: shadows clipped.';
const RED = [...CLIP_COLOURS.white, 255];
const BLUE = [...CLIP_COLOURS.black, 255];
const CLEAR = [0, 0, 0, 0];

describe('Develop: clipping overlay', () => {
  let restoreLayout: () => void;
  const saved = [...(PIXELS.saved ?? [])] as [number, number, number];
  const auto = [...(PIXELS.auto ?? [])] as [number, number, number];
  /** What the clipping layers were given to draw, newest last. */
  let drawn: Array<{ data: number[]; width: number; height: number }>;
  const newest = () => drawn[drawn.length - 1];

  beforeEach(() => {
    resetFake();
    resetLibraryStore();
    resetThumbnails();
    drawn = [];
    restoreLayout = mockLayout(900, 600);
    vi.stubGlobal('ImageData', FakeImageData);
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (
      this: HTMLCanvasElement,
    ) {
      const layer = this.dataset.layer === 'clipping';
      return {
        imageSmoothingEnabled: true,
        putImageData: (image: FakeImageData) => {
          if (layer) {
            drawn.push({ data: [...image.data], width: image.width, height: image.height });
          }
        },
      } as unknown as CanvasRenderingContext2D;
    });
    openLibrary();
    useUiStore.setState({ view: 'develop', developFrame: { id: F1, index: 0 } });
  });
  afterEach(() => {
    restoreLayout();
    PIXELS.saved = saved;
    PIXELS.auto = auto;
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  async function openFrame() {
    const user = userEvent.setup();
    const view = render(<DevelopView />);
    await waitFor(() => expect(screen.getByRole('img', { name: 'Frame 1' })).toBeInTheDocument());
    return { user, ...view };
  }

  it('is off when a frame opens, and the switch shows and hides the layer and the legend', async () => {
    const { user } = await openFrame();
    const toggle = button('Clipping');
    expect(toggle).toHaveAttribute('aria-pressed', 'false');
    expect(layers()).toHaveLength(0);
    expect(screen.queryByText(HIGHLIGHTS)).not.toBeInTheDocument();

    await user.click(toggle);
    expect(toggle).toHaveAttribute('aria-pressed', 'true');
    expect(layers()).toHaveLength(1);
    expect(screen.getByText(HIGHLIGHTS)).toBeInTheDocument();
    expect(screen.getByText(SHADOWS)).toBeInTheDocument();
    // The existing caption stays.
    expect(screen.getByText(/Black clipped: .* White clipped: /)).toBeInTheDocument();

    await user.click(toggle);
    expect(layers()).toHaveLength(0);
    expect(screen.queryByText(HIGHLIGHTS)).not.toBeInTheDocument();
  });

  it('is decorative for assistive technology, and sits over the picture at its size and place', async () => {
    const { user } = await openFrame();
    await user.click(button('Clipping'));
    const layer = layers()[0] as HTMLElement;
    expect(layer).toHaveAttribute('aria-hidden', 'true');
    expect(layer).not.toHaveAttribute('role');
    const picture = screen.getByRole('img', { name: 'Frame 1' });
    expect(layer.style.width).toBe(picture.style.width);
    expect(layer.style.height).toBe(picture.style.height);
    expect(layer.style.transform).toBe(picture.style.transform);
    expect(picture.compareDocumentPosition(layer) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('is off again when another frame opens', async () => {
    const { user } = await openFrame();
    await user.click(button('Clipping'));
    expect(layers()).toHaveLength(1);
    act(() => useUiStore.setState({ developFrame: { id: F2, index: 1 } }));
    await waitFor(() => expect(screen.getByRole('img', { name: 'Frame 2' })).toBeInTheDocument());
    expect(button('Clipping')).toHaveAttribute('aria-pressed', 'false');
    expect(layers()).toHaveLength(0);
    expect(screen.queryByText(HIGHLIGHTS)).not.toBeInTheDocument();
  });

  describe('the key', () => {
    it('J toggles it, in either case', async () => {
      const { user } = await openFrame();
      await user.keyboard('j');
      expect(button('Clipping')).toHaveAttribute('aria-pressed', 'true');
      await user.keyboard('J');
      expect(button('Clipping')).toHaveAttribute('aria-pressed', 'false');
    });

    it('is ignored while typing in a field, and with a dialog open', async () => {
      const { user } = await openFrame();
      const field = document.createElement('input');
      document.body.append(field);
      field.focus();
      await user.keyboard('j');
      field.remove();
      expect(button('Clipping')).toHaveAttribute('aria-pressed', 'false');
      await user.click(button('Export…'));
      await screen.findByRole('dialog', { name: 'Export' });
      await user.keyboard('j');
      // The dialog hides the rest of the page from the accessibility tree.
      expect(screen.getByRole('button', { name: 'Clipping', hidden: true })).toHaveAttribute(
        'aria-pressed',
        'false',
      );
    });

    it('follows the key the user chose', async () => {
      state.keys = { 'develop.toggleClipping': 'k' };
      const { user } = await openFrame();
      await waitFor(() => expect(ipc.settingsGet).toHaveBeenCalled());
      await act(async () => {});
      await user.keyboard('j');
      expect(button('Clipping')).toHaveAttribute('aria-pressed', 'false');
      await user.keyboard('k');
      expect(button('Clipping')).toHaveAttribute('aria-pressed', 'true');
    });
  });

  describe('what it is computed from', () => {
    it('paints the clipped pixels of the preview, and the caption agrees', async () => {
      // 2 x 2: white, one channel at 255, black, mid grey.
      const rgba = Uint8ClampedArray.from([255, 255, 255, 255, 10, 255, 10, 255]);
      const pixels = Uint8ClampedArray.from([...rgba, 0, 0, 0, 255, 128, 128, 128, 255]);
      ipc.developRender.mockImplementation(async (_id, _recipe, _edge, seq) => ({
        seq,
        width: 2,
        height: 2,
        rgba: pixels.slice(),
      }));
      const { user } = await openFrame();
      await user.click(button('Clipping'));
      await waitFor(() => expect(drawn).not.toHaveLength(0));
      expect([newest()?.width, newest()?.height]).toEqual([2, 2]);
      expect(newest()?.data).toEqual([...RED, ...RED, ...BLUE, ...CLEAR]);
      expect(screen.getByText('Black clipped: 25%. White clipped: 50%.')).toBeInTheDocument();
    });

    it('is built from the picture showing: the after preview, then the before one', async () => {
      PIXELS.saved = [255, 0, 0]; // after: white-clipped
      PIXELS.auto = [0, 0, 0]; // before: black-clipped
      const { user } = await openFrame();
      await user.click(button('Clipping'));
      await waitFor(() => expect(drawn.length).toBeGreaterThan(0));
      expect(newest()?.data.slice(0, 4)).toEqual(RED);
      expect(screen.getByText(/White clipped: 100%/)).toBeInTheDocument();

      await user.click(button('Before'));
      await waitFor(() => expect(newest()?.data.slice(0, 4)).toEqual(BLUE));
      expect(screen.getByText(/Black clipped: 100%\. White clipped: 0%/)).toBeInTheDocument();
    });

    it('follows a new preview: the mask is rebuilt when one arrives', async () => {
      PIXELS.saved = [255, 255, 255];
      const { user } = await openFrame();
      await user.click(button('Clipping'));
      await waitFor(() => expect(drawn.length).toBeGreaterThan(0));
      expect(newest()?.data.slice(0, 4)).toEqual(RED);
      const count = drawn.length;
      // An edit (the key Q adds red) asks for a new preview, which is black now.
      ipc.developRender.mockImplementation(async (_id, _recipe, _edge, seq) => ({
        seq,
        width: 1,
        height: 1,
        rgba: Uint8ClampedArray.from([0, 0, 0, 255]),
      }));
      await user.keyboard('q');
      await waitFor(() => expect(newest()?.data).toEqual(BLUE));
      expect(drawn.length).toBeGreaterThan(count);
    });
  });

  describe('where it shows', () => {
    it('is hidden in crop mode and back afterwards', async () => {
      const { user } = await openFrame();
      await user.click(button('Clipping'));
      expect(layers()).toHaveLength(1);
      await user.click(button('Edit crop'));
      await screen.findByRole('application', { name: /Crop area/ });
      expect(layers()).toHaveLength(0);
      // The switch keeps its state, and so does the legend.
      expect(button('Clipping')).toHaveAttribute('aria-pressed', 'true');
      await user.click(button('Edit crop'));
      await waitFor(() => expect(layers()).toHaveLength(1));
    });

    it('is on the current frame only in compare mode, not on the reference', async () => {
      const { user, container } = await openFrame();
      await user.click(button('Clipping'));
      await user.click(button('Compare with'));
      const strip = screen.getByRole('listbox', { name: 'Frames of the current Library view' });
      const frame3 = (await within(strip).findAllByRole('option')).find((o) =>
        /^Frame 3(,|$)/.test(o.getAttribute('aria-label') ?? ''),
      ) as HTMLElement;
      await user.click(frame3);
      await screen.findByRole('img', { name: 'Reference, Frame 3' });
      expect(layers()).toHaveLength(1);
      const stages = [...container.querySelectorAll('[aria-busy]')];
      expect(stages).toHaveLength(2);
      expect(stages[0]?.contains(layers()[0] as Element)).toBe(true);
      expect(stages[1]?.querySelector('[data-layer="clipping"]')).toBeNull();
    });
  });

  it('has no accessibility violations with the overlay and the legend on', async () => {
    const { user, container } = await openFrame();
    await user.click(button('Clipping'));
    await expectNoA11yViolations(container);
  });
});
