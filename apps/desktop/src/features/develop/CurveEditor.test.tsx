import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import {
  CURVE_POINTS_MAX,
  CurveEditor,
  identityCurve,
  type CurveChannel,
  type CurveEditorText,
  type CurvePoint,
  type CurveSet,
} from '@gtp/ui';
import { expectNoA11yViolations } from '../../test-support';

const text: CurveEditorText = {
  channels: 'Curve to edit',
  master: 'Master',
  red: 'Red',
  green: 'Green',
  blue: 'Blue',
  plot: (channel) => `Tone curve, ${channel}`,
  unchanged: 'This curve changes nothing.',
  add: 'Add point',
  remove: 'Remove point',
  reset: 'Reset curve',
  point: (n, total) => `Point ${n} of ${total}`,
  value: (input, output) => `Input ${input}, output ${output}`,
  number: (value) => value.toFixed(2),
};

const start = (): CurveSet => ({
  master: [
    [0, 0],
    [0.5, 0.5],
    [1, 1],
  ],
  r: [],
  g: [],
  b: [],
});

type Change = [CurveChannel, CurvePoint[], { dragging: boolean }];

/** The editor with state, as the panel has it, remembering every change it reported. */
function setup(initial: CurveSet = start(), disabled = false) {
  const changes: Change[] = [];
  const spy = vi.fn();
  function Harness() {
    const [curves, setCurves] = useState(initial);
    return (
      <CurveEditor
        curves={curves}
        text={text}
        disabled={disabled}
        onChange={(channel, points, options) => {
          changes.push([channel, points, options]);
          spy(channel, points, options);
          setCurves((c) => ({ ...c, [channel]: points }));
        }}
      />
    );
  }
  return { changes, ...render(<Harness />) };
}

const point = (n: number, total: number) =>
  screen.getByRole('slider', { name: `Point ${n} of ${total}` });
const last = (changes: Change[]) => changes[changes.length - 1] as Change;

describe('CurveEditor', () => {
  it('shows the points of the master curve, with their values', () => {
    setup();
    expect(screen.getByRole('group', { name: 'Tone curve, Master' })).toBeInTheDocument();
    expect(point(1, 3)).toHaveAttribute('aria-valuetext', 'Input 0.00, output 0.00');
    expect(point(2, 3)).toHaveAttribute('aria-valuetext', 'Input 0.50, output 0.50');
    expect(point(2, 3)).toHaveAttribute('aria-valuenow', '0.5');
    expect(screen.getByRole('button', { name: 'Master' })).toHaveAttribute('aria-pressed', 'true');
  });

  describe('with the keyboard', () => {
    it('moves a point by 0.01 with the arrow keys and 0.1 with Shift', async () => {
      const user = userEvent.setup();
      const { changes } = setup();
      point(2, 3).focus();
      await user.keyboard('{ArrowRight}');
      expect(last(changes)[0]).toBe('master');
      expect(last(changes)[1][1]).toEqual([0.51, 0.5]);
      expect(last(changes)[2]).toEqual({ dragging: false });

      await user.keyboard('{ArrowUp}{ArrowUp}');
      expect(last(changes)[1][1]?.[1]).toBeCloseTo(0.52, 9);
      await user.keyboard('{ArrowDown}');
      expect(last(changes)[1][1]?.[1]).toBeCloseTo(0.51, 9);
      await user.keyboard('{Shift>}{ArrowLeft}{/Shift}');
      expect(last(changes)[1][1]?.[0]).toBeCloseTo(0.41, 9);
      expect(point(2, 3)).toHaveAttribute('aria-valuetext', 'Input 0.41, output 0.51');
    });

    it('keeps a point in the square and between its neighbours', async () => {
      const user = userEvent.setup();
      const { changes } = setup();
      point(1, 3).focus();
      await user.keyboard('{ArrowLeft}{ArrowDown}');
      expect(last(changes)[1][0]).toEqual([0, 0]);
      point(3, 3).focus();
      await user.keyboard('{ArrowRight}{ArrowUp}');
      expect(last(changes)[1][2]).toEqual([1, 1]);
      point(2, 3).focus();
      await user.keyboard(
        '{Shift>}{ArrowRight}{ArrowRight}{ArrowRight}{ArrowRight}{ArrowRight}{/Shift}',
      );
      const x = last(changes)[1][1]?.[0] ?? 0;
      expect(x).toBeLessThan(1);
      expect(x).toBeGreaterThan(0.9);
    });

    it('removes the focused point with Delete, and focus goes to a neighbour', async () => {
      const user = userEvent.setup();
      const { changes } = setup();
      point(2, 3).focus();
      await user.keyboard('{Delete}');
      expect(last(changes)[1]).toEqual([
        [0, 0],
        [1, 1],
      ]);
      expect(point(2, 2)).toHaveFocus();

      // Two points are the least a curve has.
      await user.keyboard('{Delete}');
      expect(changes).toHaveLength(1);
      expect(screen.getByRole('button', { name: 'Remove point' })).toBeDisabled();
    });

    it('adds a point with its button, in the widest gap, and focuses it', async () => {
      const user = userEvent.setup();
      const { changes } = setup();
      await user.click(screen.getByRole('button', { name: 'Add point' }));
      expect(last(changes)[1]).toHaveLength(4);
      expect(last(changes)[1].map((p) => p[0])).toEqual([0, 0.25, 0.5, 1]);
      expect(point(2, 4)).toHaveFocus();
      // It sits on the curve, so the picture does not change when it is added.
      expect(last(changes)[1][1]?.[1]).toBeCloseTo(0.25, 6);
    });

    it('does not add past 16 points', async () => {
      const user = userEvent.setup();
      const { changes } = setup();
      for (let i = 0; i < CURVE_POINTS_MAX + 3; i += 1) {
        await user.click(screen.getByRole('button', { name: 'Add point' }));
      }
      expect(last(changes)[1]).toHaveLength(CURVE_POINTS_MAX);
      expect(changes).toHaveLength(CURVE_POINTS_MAX - 3);
    });

    it('removes the selected point with its button', async () => {
      const user = userEvent.setup();
      const { changes } = setup();
      expect(screen.getByRole('button', { name: 'Remove point' })).toBeDisabled();
      point(2, 3).focus();
      await user.click(screen.getByRole('button', { name: 'Remove point' }));
      expect(last(changes)[1]).toHaveLength(2);
    });
  });

  describe('with the pointer', () => {
    // jsdom does no layout: the drawing is 272 units across, shown 272 pixels wide at (0, 0).
    const layout = () =>
      vi
        .spyOn(Element.prototype, 'getBoundingClientRect')
        .mockImplementation(
          () => ({ left: 0, top: 0, width: 272, height: 272, right: 272, bottom: 272 }) as DOMRect,
        );

    it('drags a point, reporting a drag while it moves and the release at the end', () => {
      layout();
      const { changes } = setup();
      const dot = point(2, 3);
      // Unit (0.75, 0.25) is at x = 8 + 192 and y = 8 + 192 in the drawing.
      fireEvent.pointerDown(dot, { button: 0, pointerId: 1 });
      fireEvent.pointerMove(dot, { clientX: 200, clientY: 200, pointerId: 1 });
      expect(last(changes)[2]).toEqual({ dragging: true });
      expect(last(changes)[1][1]?.[0]).toBeCloseTo(0.75, 6);
      expect(last(changes)[1][1]?.[1]).toBeCloseTo(0.25, 6);
      fireEvent.pointerMove(dot, { clientX: 100, clientY: 100, pointerId: 1 });
      fireEvent.pointerUp(dot, { pointerId: 1 });
      expect(last(changes)[2]).toEqual({ dragging: false });
      expect(last(changes)[1][1]?.[0]).toBeCloseTo((100 - 8) / 256, 6);
      vi.restoreAllMocks();
    });

    it('does not move anything on a mouse move without a press', () => {
      layout();
      const { changes } = setup();
      fireEvent.pointerMove(point(2, 3), { clientX: 200, clientY: 200 });
      expect(changes).toHaveLength(0);
      vi.restoreAllMocks();
    });

    it('adds a point where the plot is double-clicked', () => {
      layout();
      const { changes, container } = setup();
      const field = container.querySelector('rect') as SVGRectElement;
      fireEvent.doubleClick(field, { clientX: 8 + 64, clientY: 8 + 192 });
      expect(last(changes)[1]).toHaveLength(4);
      expect(last(changes)[1][1]?.[0]).toBeCloseTo(0.25, 6);
      expect(last(changes)[1][1]?.[1]).toBeCloseTo(0.25, 6);
      vi.restoreAllMocks();
    });
  });

  describe('the colour curves', () => {
    it('start with no points, saying they change nothing', async () => {
      const user = userEvent.setup();
      setup();
      await user.click(screen.getByRole('button', { name: 'Red' }));
      expect(screen.getByRole('group', { name: 'Tone curve, Red' })).toBeInTheDocument();
      expect(screen.queryAllByRole('slider')).toHaveLength(0);
      expect(screen.getByText('This curve changes nothing.')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Reset curve' })).toBeDisabled();
    });

    it('start from the straight line when the first point is added, and reset to nothing', async () => {
      const user = userEvent.setup();
      const { changes } = setup();
      await user.click(screen.getByRole('button', { name: 'Green' }));
      await user.click(screen.getByRole('button', { name: 'Add point' }));
      expect(last(changes)[0]).toBe('g');
      expect(last(changes)[1]).toEqual([
        [0, 0],
        [0.5, 0.5],
        [1, 1],
      ]);
      expect(screen.queryByText('This curve changes nothing.')).not.toBeInTheDocument();

      await user.click(screen.getByRole('button', { name: 'Reset curve' }));
      expect(last(changes)[1]).toEqual([]);
      expect(screen.getByText('This curve changes nothing.')).toBeInTheDocument();
    });

    it('leave the other curves alone', async () => {
      const user = userEvent.setup();
      const { changes } = setup();
      await user.click(screen.getByRole('button', { name: 'Blue' }));
      await user.click(screen.getByRole('button', { name: 'Add point' }));
      await user.click(screen.getByRole('button', { name: 'Master' }));
      expect(point(2, 3)).toBeInTheDocument();
      expect(changes.every(([channel]) => channel === 'b')).toBe(true);
    });
  });

  it('resets the master curve to the straight line', async () => {
    const user = userEvent.setup();
    const { changes } = setup();
    await user.click(screen.getByRole('button', { name: 'Reset curve' }));
    expect(last(changes)[1]).toEqual(identityCurve());
  });

  it('can be looked at but not changed when disabled', async () => {
    const user = userEvent.setup();
    const { changes } = setup(start(), true);
    expect(screen.getByRole('button', { name: 'Add point' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Reset curve' })).toBeDisabled();
    point(2, 3).focus();
    await user.keyboard('{ArrowRight}{Delete}');
    expect(changes).toHaveLength(0);
  });

  it('has no accessibility violations, with points and without', async () => {
    const user = userEvent.setup();
    const { container } = setup();
    await expectNoA11yViolations(container);
    await user.click(screen.getByRole('button', { name: 'Red' }));
    await expectNoA11yViolations(container);
  });
});
