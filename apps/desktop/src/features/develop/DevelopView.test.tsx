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

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

/** A library with the roll "Summer" (5 frames) open and chosen, as the Library leaves it. */
function openLibrary() {
  state.details = libraryDetails();
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
      expect(calls[calls.length - 1]?.[1]).toEqual({ v: 1, tag: 'auto' });

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
  });
});
