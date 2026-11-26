import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../test-support';
import { ipc, makeRoll, resetFake, state } from '../../library/fake-ipc';
import type { Roll } from '../../../shared/ipc';
import { LiveView } from './LiveView';
import { defaultRect } from './guides';

vi.mock('../../../shared/ipc', async () => (await import('../../library/fake-ipc')).ipc);

// The live view reads the roll from the rig; the rig itself is not needed here.
const rigRoll: { current: Roll | null } = { current: null };
vi.mock('./CaptureRig', () => ({
  useOptionalCaptureRig: () => (rigRoll.current ? { roll: rigRoll.current } : null),
}));

type User = ReturnType<typeof userEvent.setup>;

const onArea = vi.fn();

function View() {
  return (
    <LiveView
      calibrationArea={null}
      onCalibrationAreaChange={onArea}
      levels={[10, 10, 10]}
      lightLost={false}
      blocked={false}
      stopRef={{ current: null }}
    />
  );
}

async function started(user: User) {
  render(
    <main>
      <View />
    </main>,
  );
  await user.click(screen.getByRole('button', { name: 'Start live view' }));
  await screen.findByRole('img', { name: 'Live view of the camera' });
  // The zoom steps have arrived.
  await screen.findByRole('radio', { name: '25 %' });
}

const frameLines = () => screen.queryByTestId('live-view-frame-lines');
const grid = () => screen.queryByTestId('live-view-grid');
const frameSwitch = () => screen.getByRole('checkbox', { name: 'Frame lines' });
const gridSwitch = () => screen.getByRole('checkbox', { name: 'Grid' });

/** Gives the overlay a size, as jsdom has no layout: 400 x 200 pixels at the origin. */
function sizeOverlay() {
  const box = screen.getByTestId('live-view-guides-drawing').parentElement as HTMLElement;
  box.getBoundingClientRect = () =>
    ({ left: 0, top: 0, width: 400, height: 200, right: 400, bottom: 200, x: 0, y: 0 }) as DOMRect;
}

beforeEach(() => {
  resetFake();
  rigRoll.current = makeRoll({ format: '35mm' });
  vi.clearAllMocks();
});
afterEach(cleanup);

describe('frame lines and grid in the live view', () => {
  it('has both switches off at first, so nothing is drawn', async () => {
    const user = userEvent.setup();
    await started(user);
    expect(frameSwitch()).not.toBeChecked();
    expect(gridSwitch()).not.toBeChecked();
    expect(frameLines()).toBeNull();
    expect(grid()).toBeNull();
    expect(screen.queryByRole('button', { name: 'Adjust frame lines' })).toBeNull();
  });

  it('draws frame lines with the format proportions and the grid with its kind', async () => {
    const user = userEvent.setup();
    await started(user);
    await user.click(frameSwitch());
    const lines = frameLines() as HTMLElement;
    const want = defaultRect({
      format: '35mm',
      frameSize: null,
      orientation: 'landscape',
      pictureAspect: 1.5,
    });
    expect(parseFloat(lines.style.width)).toBeCloseTo(want.w * 100, 3);
    expect(parseFloat(lines.style.height)).toBeCloseTo(want.h * 100, 3);
    await user.click(gridSwitch());
    expect(grid()).toHaveAttribute('data-kind', 'thirds');
    await user.click(screen.getByRole('radio', { name: 'Fine' }));
    expect(grid()).toHaveAttribute('data-kind', 'fine');
  });

  it('uses the roll frame size when it has one, and its format otherwise', async () => {
    rigRoll.current = makeRoll({ format: '6x6', frame_size: { w: 0.5, h: 0.5, samples: 0 } });
    const user = userEvent.setup();
    await started(user);
    await user.click(frameSwitch());
    const lines = frameLines() as HTMLElement;
    // 0.5 x 0.5 of the picture, not the square the format would give.
    expect(parseFloat(lines.style.width)).toBeCloseTo(50, 3);
    expect(parseFloat(lines.style.height)).toBeCloseTo(50, 3);
  });

  it('turns the lines to portrait and back', async () => {
    const user = userEvent.setup();
    await started(user);
    await user.click(frameSwitch());
    const before = frameLines() as HTMLElement;
    const landscapeWidth = parseFloat(before.style.width);
    await user.click(screen.getByRole('radio', { name: 'Portrait' }));
    const portrait = frameLines() as HTMLElement;
    expect(parseFloat(portrait.style.width)).toBeLessThan(landscapeWidth);
    expect(parseFloat(portrait.style.height)).toBeGreaterThan(parseFloat(portrait.style.width));
    await user.click(screen.getByRole('radio', { name: 'Landscape' }));
    expect(parseFloat((frameLines() as HTMLElement).style.width)).toBeCloseTo(landscapeWidth, 3);
  });

  it('saves the choices in the settings and reads them back', async () => {
    const user = userEvent.setup();
    await started(user);
    await user.click(frameSwitch());
    await user.click(gridSwitch());
    await user.click(screen.getByRole('radio', { name: 'Fine' }));
    await waitFor(() => expect(state.liveViewGuides.grid).toBe('fine'));
    expect(state.liveViewGuides.frame_lines).toBe(true);
    expect(ipc.settingsSet).toHaveBeenLastCalledWith({
      live_view_guides: { frame_lines: true, grid: 'fine', adjusted: {} },
    });
    cleanup();

    // Another time the live view is opened, the choices are back.
    await started(user);
    await waitFor(() => expect(frameSwitch()).toBeChecked());
    expect(gridSwitch()).toBeChecked();
    expect(screen.getByRole('radio', { name: 'Fine' })).toBeChecked();
    expect(frameLines()).not.toBeNull();
    expect(grid()).toHaveAttribute('data-kind', 'fine');
  });

  it('says so when the choices cannot be saved', async () => {
    state.settingsRefused = 'Settings are read-only.';
    const user = userEvent.setup();
    await started(user);
    await user.click(frameSwitch());
    expect(await screen.findByRole('alert')).toHaveTextContent('Settings are read-only.');
  });

  it('hides both while the camera is zoomed in, with a note, and shows them again', async () => {
    const user = userEvent.setup();
    await started(user);
    await user.click(frameSwitch());
    await user.click(gridSwitch());
    expect(frameLines()).not.toBeNull();
    await user.click(screen.getByRole('radio', { name: '25 %' }));
    await waitFor(() => expect(frameLines()).toBeNull());
    expect(grid()).toBeNull();
    expect(screen.getByText(/hidden while the camera is zoomed in/)).toBeInTheDocument();
    await user.click(screen.getByRole('radio', { name: 'Whole frame' }));
    await waitFor(() => expect(frameLines()).not.toBeNull());
    expect(grid()).not.toBeNull();
    expect(screen.queryByText(/hidden while the camera is zoomed in/)).toBeNull();
  });

  it('lets a click through to aim the zoom while the lines are not adjusted', async () => {
    const user = userEvent.setup();
    await started(user);
    await user.click(frameSwitch());
    await user.click(gridSwitch());
    // The drawing is for the eye only: nothing on it takes a press or can be focused.
    expect(frameLines()).toHaveAttribute('aria-hidden', 'true');
    expect(frameLines()).not.toHaveAttribute('tabindex');
    fireEvent.pointerDown(frameLines() as HTMLElement, { clientX: 20, clientY: 20 });
    fireEvent.pointerUp(frameLines() as HTMLElement, { clientX: 20, clientY: 20 });
    expect(ipc.settingsSet).toHaveBeenCalledTimes(2);
    await user.click(
      screen.getByRole('button', {
        name: /Choose the spot to zoom to/,
      }),
    );
    await waitFor(() => expect(ipc.cameraPreviewSetArea).toHaveBeenCalledTimes(1));
    expect(onArea).not.toHaveBeenCalled();
  });

  it('adjusts with the mouse without drawing a calibration area or aiming the zoom', async () => {
    const user = userEvent.setup();
    await started(user);
    await user.click(frameSwitch());
    const button = screen.getByRole('button', { name: 'Adjust frame lines' });
    expect(button).toHaveAttribute('aria-pressed', 'false');
    await user.click(button);
    expect(button).toHaveAttribute('aria-pressed', 'true');
    sizeOverlay();
    const lines = screen.getByRole('group', { name: /Frame lines\. Arrow keys/ });

    // Move: a drag of 40 px across a 400 px picture is 10 %.
    fireEvent.pointerDown(lines, { clientX: 200, clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(lines, { clientX: 220, clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(lines, { clientX: 240, clientY: 100, pointerId: 1 });
    fireEvent.pointerUp(lines, { clientX: 240, clientY: 100, pointerId: 1 });
    await waitFor(() =>
      expect(Object.keys(state.liveViewGuides.adjusted)).toEqual(['35mm-landscape']),
    );
    const moved = state.liveViewGuides.adjusted['35mm-landscape'];
    // 0.05 + 0.1 would stick out of the picture (the lines are 90 % wide): it stops at the edge.
    expect(moved?.x).toBeCloseTo(0.1, 3);
    expect(moved?.w).toBeCloseTo(0.9, 3);

    // Resize: drag the bottom right corner to the left.
    const before = state.liveViewGuides.adjusted['35mm-landscape'];
    const corner = screen.getByTestId('live-view-handle-se');
    fireEvent.pointerDown(corner, { clientX: 390, clientY: 190, pointerId: 2 });
    fireEvent.pointerMove(corner, { clientX: 300, clientY: 150, pointerId: 2 });
    fireEvent.pointerUp(corner, { clientX: 300, clientY: 150, pointerId: 2 });
    await waitFor(() =>
      expect(state.liveViewGuides.adjusted['35mm-landscape']?.w).toBeLessThan(before?.w ?? 0),
    );
    expect(state.liveViewGuides.adjusted['35mm-landscape']?.x).toBeCloseTo(before?.x ?? 0, 5);

    // None of it was a calibration drag or a click on the picture.
    expect(onArea).not.toHaveBeenCalled();
    expect(ipc.cameraPreviewSetArea).not.toHaveBeenCalled();
  });

  it('adjusts with the keyboard, keeps it per orientation, and resets', async () => {
    const user = userEvent.setup();
    await started(user);
    await user.click(frameSwitch());
    await user.click(screen.getByRole('button', { name: 'Adjust frame lines' }));
    const reset = screen.getByRole('button', { name: 'Reset frame lines' });
    expect(reset).toBeDisabled();
    const lines = screen.getByRole('group', { name: /Frame lines\. Arrow keys/ });
    const x0 = parseFloat(lines.style.left);
    const w0 = parseFloat(lines.style.width);
    lines.focus();
    await user.keyboard('{ArrowLeft}{ArrowLeft}');
    await waitFor(() =>
      expect(
        parseFloat(screen.getByRole('group', { name: /Arrow keys move/ }).style.left),
      ).toBeCloseTo(x0 - 1, 3),
    );
    await user.keyboard('{Shift>}{ArrowLeft}{/Shift}');
    await waitFor(() =>
      expect(
        parseFloat(screen.getByRole('group', { name: /Arrow keys move/ }).style.width),
      ).toBeCloseTo(w0 - 1, 3),
    );
    expect(reset).toBeEnabled();
    expect(Object.keys(state.liveViewGuides.adjusted)).toEqual(['35mm-landscape']);

    // Portrait has its own lines, still the default.
    await user.click(screen.getByRole('radio', { name: 'Portrait' }));
    expect(screen.getByRole('button', { name: 'Reset frame lines' })).toBeDisabled();
    await user.click(screen.getByRole('radio', { name: 'Landscape' }));

    await user.click(screen.getByRole('button', { name: 'Reset frame lines' }));
    await waitFor(() => expect(state.liveViewGuides.adjusted).toEqual({}));
    expect(
      parseFloat(screen.getByRole('group', { name: /Arrow keys move/ }).style.left),
    ).toBeCloseTo(x0, 3);
    expect(
      parseFloat(screen.getByRole('group', { name: /Arrow keys move/ }).style.width),
    ).toBeCloseTo(w0, 3);
  });

  it('keeps the adjusted lines inside the picture and not smaller than the minimum', async () => {
    const user = userEvent.setup();
    await started(user);
    await user.click(frameSwitch());
    await user.click(screen.getByRole('button', { name: 'Adjust frame lines' }));
    sizeOverlay();
    const lines = screen.getByRole('group', { name: /Arrow keys move/ });
    fireEvent.pointerDown(lines, { clientX: 200, clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(lines, { clientX: 4000, clientY: 4000, pointerId: 1 });
    fireEvent.pointerUp(lines, { clientX: 4000, clientY: 4000, pointerId: 1 });
    await waitFor(() => expect(state.liveViewGuides.adjusted['35mm-landscape']).toBeDefined());
    const kept = state.liveViewGuides.adjusted['35mm-landscape'];
    expect((kept?.x ?? 0) + (kept?.w ?? 0)).toBeLessThanOrEqual(1 + 1e-6);
    expect((kept?.y ?? 0) + (kept?.h ?? 0)).toBeLessThanOrEqual(1 + 1e-6);

    const corner = screen.getByTestId('live-view-handle-nw');
    fireEvent.pointerDown(corner, { clientX: 0, clientY: 0, pointerId: 2 });
    fireEvent.pointerMove(corner, { clientX: 4000, clientY: 4000, pointerId: 2 });
    fireEvent.pointerUp(corner, { clientX: 4000, clientY: 4000, pointerId: 2 });
    await waitFor(() =>
      expect(state.liveViewGuides.adjusted['35mm-landscape']?.w).toBeCloseTo(0.05, 3),
    );
    expect(state.liveViewGuides.adjusted['35mm-landscape']?.h).toBeCloseTo(0.05, 3);
  });

  it('is axe clean with everything on', async () => {
    const user = userEvent.setup();
    await started(user);
    await user.click(frameSwitch());
    await user.click(gridSwitch());
    await user.click(screen.getByRole('button', { name: 'Adjust frame lines' }));
    await expectNoA11yViolations(document.body);
  });
});
