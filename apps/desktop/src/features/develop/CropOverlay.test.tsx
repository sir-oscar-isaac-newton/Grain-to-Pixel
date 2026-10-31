import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../test-support';
import { BIG_NUDGE, NUDGE, type CropRect } from './crop';
import { CropOverlay } from './CropOverlay';

type Change = [CropRect, { dragging: boolean }];

/** The overlay with state, as the view has it, remembering every change it reported. */
function setup(initial: CropRect = { x: 0.2, y: 0.2, w: 0.5, h: 0.4 }, disabled = false) {
  const changes: Change[] = [];
  function Harness() {
    const [crop, setCrop] = useState(initial);
    return (
      <CropOverlay
        crop={crop}
        style={{ width: 400, height: 300 }}
        disabled={disabled}
        label="Crop area"
        onChange={(next, options) => {
          changes.push([next, options]);
          setCrop(next);
        }}
      />
    );
  }
  return { changes, ...render(<Harness />) };
}

const area = () => screen.getByRole('application', { name: 'Crop area' });
const last = (changes: Change[]) => changes[changes.length - 1] as Change;

/** jsdom does no layout: the picture is 400 x 300 pixels at (100, 50). */
const layout = () =>
  vi
    .spyOn(Element.prototype, 'getBoundingClientRect')
    .mockImplementation(
      () => ({ left: 100, top: 50, width: 400, height: 300, right: 500, bottom: 350 }) as DOMRect,
    );
/** The screen position of a point given as fractions of the picture. */
const screenAt = (x: number, y: number) => ({ clientX: 100 + x * 400, clientY: 50 + y * 300 });

afterEach(() => vi.restoreAllMocks());

describe('CropOverlay', () => {
  it('draws the crop where the recipe says, as a share of the picture', () => {
    setup();
    expect(area()).toHaveStyle({ left: '20%', top: '20%', width: '50%', height: '40%' });
  });

  describe('with the keyboard', () => {
    it('moves the crop one step per arrow key and a larger one with Shift', async () => {
      const user = userEvent.setup();
      const { changes } = setup();
      area().focus();
      await user.keyboard('{ArrowRight}');
      expect(last(changes)[0].x).toBeCloseTo(0.2 + NUDGE, 9);
      expect(last(changes)[1]).toEqual({ dragging: false });
      await user.keyboard('{ArrowDown}{ArrowDown}');
      expect(last(changes)[0].y).toBeCloseTo(0.2 + 2 * NUDGE, 9);
      await user.keyboard('{Shift>}{ArrowLeft}{/Shift}');
      expect(last(changes)[0].x).toBeCloseTo(0.2 + NUDGE - BIG_NUDGE, 9);
      await user.keyboard('{Shift>}{ArrowUp}{/Shift}');
      expect(last(changes)[0].y).toBeCloseTo(0.2 + 2 * NUDGE - BIG_NUDGE, 9);
      // The size is never changed by moving.
      expect([last(changes)[0].w, last(changes)[0].h]).toEqual([0.5, 0.4]);
    });

    it('stops at the edge of the scan', async () => {
      const user = userEvent.setup();
      const { changes } = setup({ x: 0.5, y: 0.6, w: 0.5, h: 0.4 });
      area().focus();
      await user.keyboard('{ArrowRight}{ArrowDown}');
      expect(last(changes)[0]).toEqual({ x: 0.5, y: 0.6, w: 0.5, h: 0.4 });
    });

    it('ignores other keys and keys with Ctrl', async () => {
      const user = userEvent.setup();
      const { changes } = setup();
      area().focus();
      await user.keyboard('a{Enter}{Control>}{ArrowRight}{/Control}');
      expect(changes).toHaveLength(0);
    });
  });

  describe('with the pointer', () => {
    it('moves the crop by dragging it, reporting a drag and then the release', () => {
      layout();
      const { changes } = setup();
      const dragged = area();
      fireEvent.pointerDown(dragged, { button: 0, pointerId: 1, ...screenAt(0.4, 0.4) });
      fireEvent.pointerMove(dragged, { pointerId: 1, ...screenAt(0.5, 0.45) });
      expect(last(changes)[1]).toEqual({ dragging: true });
      expect(last(changes)[0].x).toBeCloseTo(0.3, 6);
      expect(last(changes)[0].y).toBeCloseTo(0.25, 6);
      fireEvent.pointerMove(dragged, { pointerId: 1, ...screenAt(0.2, 0.4) });
      fireEvent.pointerUp(dragged, { pointerId: 1 });
      expect(last(changes)[1]).toEqual({ dragging: false });
      // It followed the pointer from where the press was, not from the last move.
      expect(last(changes)[0].x).toBeCloseTo(0, 6);
      expect(last(changes)[0].y).toBeCloseTo(0.2, 6);
    });

    it('stops a drag at the edge of the scan', () => {
      layout();
      const { changes } = setup();
      fireEvent.pointerDown(area(), { button: 0, pointerId: 1, ...screenAt(0.4, 0.4) });
      fireEvent.pointerMove(area(), { pointerId: 1, ...screenAt(1, 1) });
      fireEvent.pointerUp(area(), { pointerId: 1 });
      expect(last(changes)[0]).toMatchObject({ x: 0.5, y: 0.6, w: 0.5, h: 0.4 });
    });

    it.each([
      ['e', { x: 0.2, y: 0.2, w: 0.7, h: 0.4 }, screenAt(0.9, 0.9)],
      ['w', { x: 0.1, y: 0.2, w: 0.6, h: 0.4 }, screenAt(0.1, 0.9)],
      ['n', { x: 0.2, y: 0.1, w: 0.5, h: 0.5 }, screenAt(0.9, 0.1)],
      ['s', { x: 0.2, y: 0.2, w: 0.5, h: 0.6 }, screenAt(0.9, 0.8)],
      ['se', { x: 0.2, y: 0.2, w: 0.7, h: 0.6 }, screenAt(0.9, 0.8)],
      ['nw', { x: 0.1, y: 0.1, w: 0.6, h: 0.5 }, screenAt(0.1, 0.1)],
      ['ne', { x: 0.2, y: 0.1, w: 0.7, h: 0.5 }, screenAt(0.9, 0.1)],
      ['sw', { x: 0.1, y: 0.2, w: 0.6, h: 0.6 }, screenAt(0.1, 0.8)],
    ] as const)('resizes by the %s handle', (handle, want, to) => {
      layout();
      const { changes, container } = setup();
      const grip = container.querySelector(`[data-handle="${handle}"]`) as HTMLElement;
      fireEvent.pointerDown(grip, { button: 0, pointerId: 1, ...screenAt(0, 0) });
      fireEvent.pointerMove(grip, { pointerId: 1, ...to });
      fireEvent.pointerUp(grip, { pointerId: 1 });
      const got = last(changes)[0];
      for (const key of ['x', 'y', 'w', 'h'] as const) expect(got[key]).toBeCloseTo(want[key], 6);
      expect(last(changes)[1]).toEqual({ dragging: false });
    });

    it('does nothing for another mouse button, or a move without a press', () => {
      layout();
      const { changes } = setup();
      fireEvent.pointerDown(area(), { button: 2, pointerId: 1, ...screenAt(0.4, 0.4) });
      fireEvent.pointerMove(area(), { pointerId: 1, ...screenAt(0.5, 0.5) });
      fireEvent.pointerUp(area(), { pointerId: 1 });
      fireEvent.pointerMove(area(), { pointerId: 1, ...screenAt(0.6, 0.6) });
      expect(changes).toHaveLength(0);
    });
  });

  it('cannot be changed when disabled', async () => {
    layout();
    const user = userEvent.setup();
    const { changes } = setup(undefined, true);
    expect(area()).toHaveAttribute('tabindex', '-1');
    fireEvent.pointerDown(area(), { button: 0, pointerId: 1, ...screenAt(0.4, 0.4) });
    fireEvent.pointerMove(area(), { pointerId: 1, ...screenAt(0.6, 0.6) });
    fireEvent.pointerUp(area(), { pointerId: 1 });
    area().focus();
    await user.keyboard('{ArrowRight}');
    expect(changes).toHaveLength(0);
  });

  it('has no accessibility violations', async () => {
    const { container } = setup();
    await expectNoA11yViolations(container);
  });
});
