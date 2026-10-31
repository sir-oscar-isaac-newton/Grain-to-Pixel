import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useRef } from 'react';
import type { Fractions } from './picker';
import { usePicker } from './use-picker';

/** A stage of 600 x 400 at (0, 0) holding a picture of 400 x 300 at (100, 50). */
function Harness({ active, onPick }: { active: boolean; onPick: (region: Fractions) => void }) {
  const stage = useRef<HTMLDivElement>(null);
  const box = usePicker({ stageRef: stage, active, onPick });
  return (
    <div ref={stage} data-testid="stage">
      <canvas data-testid="picture" />
      {box && <div data-testid="box" style={box} />}
    </div>
  );
}

const rect = (left: number, top: number, width: number, height: number) =>
  ({
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    x: left,
    y: top,
  }) as DOMRect;

describe('usePicker', () => {
  beforeEach(() => {
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: HTMLElement,
    ) {
      return this.tagName === 'CANVAS' ? rect(100, 50, 400, 300) : rect(0, 0, 600, 400);
    });
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  const press = (x: number, y: number, button = 0) =>
    fireEvent.mouseDown(screen.getByTestId('stage'), { button, clientX: x, clientY: y });
  const move = (x: number, y: number) => fireEvent.mouseMove(window, { clientX: x, clientY: y });
  const release = (x: number, y: number) => fireEvent.mouseUp(window, { clientX: x, clientY: y });

  it('takes a click as a small square around it, in fractions of the picture', () => {
    const onPick = vi.fn();
    render(<Harness active onPick={onPick} />);
    press(300, 200);
    release(300, 200);
    expect(onPick).toHaveBeenCalledTimes(1);
    const region = onPick.mock.calls[0]?.[0] as Fractions;
    expect(region.x + region.w / 2).toBeCloseTo(0.5, 6);
    expect(region.y + region.h / 2).toBeCloseTo(0.5, 6);
    expect(region.w).toBeCloseTo(0.02, 6);
  });

  it('forgives a few pixels of wobble in a click', () => {
    const onPick = vi.fn();
    render(<Harness active onPick={onPick} />);
    press(300, 200);
    move(302, 201);
    expect(screen.queryByTestId('box')).not.toBeInTheDocument();
    release(302, 201);
    expect(onPick).toHaveBeenCalledTimes(1);
    expect((onPick.mock.calls[0]?.[0] as Fractions).w).toBeCloseTo(0.02, 6);
  });

  it('takes a dragged box as its own region, and shows the box while it is dragged', () => {
    const onPick = vi.fn();
    render(<Harness active onPick={onPick} />);
    press(200, 100);
    move(300, 200);
    const box = screen.getByTestId('box');
    expect(box).toHaveStyle({ left: '200px', top: '100px', width: '100px', height: '100px' });
    release(300, 200);
    expect(screen.queryByTestId('box')).not.toBeInTheDocument();
    expect(onPick).toHaveBeenCalledWith({ x: 0.25, y: 50 / 300, w: 0.25, h: 100 / 300 });
  });

  it('does nothing for a press outside the picture, with another button, or when it is off', () => {
    const onPick = vi.fn();
    const { rerender } = render(<Harness active onPick={onPick} />);
    press(20, 20);
    release(20, 20);
    press(300, 200, 2);
    release(300, 200);
    expect(onPick).not.toHaveBeenCalled();

    rerender(<Harness active={false} onPick={onPick} />);
    press(300, 200);
    release(300, 200);
    expect(onPick).not.toHaveBeenCalled();
  });

  it('picks nothing from a box that ends up off the picture', () => {
    const onPick = vi.fn();
    render(<Harness active onPick={onPick} />);
    press(200, 100);
    move(50, 20);
    release(50, 20);
    // The part of the box on the picture is a box too: from the press to the picture's corner.
    expect(onPick).toHaveBeenCalledWith({ x: 0, y: 0, w: 0.25, h: 50 / 300 });
  });

  it('stops listening when it is turned off, also in the middle of a drag', () => {
    const onPick = vi.fn();
    const { rerender } = render(<Harness active onPick={onPick} />);
    press(200, 100);
    move(300, 200);
    expect(screen.getByTestId('box')).toBeInTheDocument();
    act(() => rerender(<Harness active={false} onPick={onPick} />));
    expect(screen.queryByTestId('box')).not.toBeInTheDocument();
    press(300, 200);
    release(300, 200);
    expect(onPick).not.toHaveBeenCalled();
  });

  it('uses the newest callback, not the one it started with', () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = render(<Harness active onPick={first} />);
    rerender(<Harness active onPick={second} />);
    press(300, 200);
    release(300, 200);
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });
});
