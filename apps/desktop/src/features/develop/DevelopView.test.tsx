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
import { SAVE_DELAY_MS, WHOLE_SCAN_GEOMETRY } from './use-develop';

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
      expect(await screen.findAllByRole('option')).toHaveLength(5);
    });

    it('opens another frame when it is clicked', async () => {
      const user = userEvent.setup();
      openLibrary();
      open(1);
      render(<DevelopView />);
      await canvas();
      const options = await screen.findAllByRole('option');
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
