import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { Slider } from '@gtp/ui';
import { expectNoA11yViolations } from '../../test-support';

describe('Slider', () => {
  const setup = (over: Partial<Parameters<typeof Slider>[0]> = {}) => {
    const onChange = vi.fn();
    const view = render(
      <Slider
        label="Black point, red"
        value={0.5}
        min={0}
        max={4}
        step={0.1}
        onChange={onChange}
        {...over}
      />,
    );
    return { onChange, ...view };
  };

  it('is a slider named by its label, with its value written beside it', () => {
    setup({ format: (v) => `${v.toFixed(2)} stops` });
    const slider = screen.getByRole('slider', { name: 'Black point, red' });
    expect(slider).toHaveAttribute('aria-valuenow', '0.5');
    expect(slider).toHaveAttribute('aria-valuemin', '0');
    expect(slider).toHaveAttribute('aria-valuemax', '4');
    expect(slider).toHaveAttribute('aria-valuetext', '0.50 stops');
    expect(screen.getByText('0.50 stops')).toBeInTheDocument();
  });

  it('reports a key press as a release at once, so a screen shows the sharp result straight away', async () => {
    const user = userEvent.setup();
    const { onChange } = setup();
    screen.getByRole('slider').focus();
    await user.keyboard('{ArrowRight}');
    // Radix reports the release before the move; the move after it is not "still dragging".
    expect(onChange.mock.calls).toEqual([[0.6, false]]);

    onChange.mockClear();
    await user.keyboard('{End}');
    expect(onChange.mock.calls).toEqual([[4, false]]);
  });
  it('stays within its range', async () => {
    const user = userEvent.setup();
    const { onChange } = setup({ value: 0 });
    screen.getByRole('slider').focus();
    await user.keyboard('{ArrowLeft}');
    expect(onChange).not.toHaveBeenCalledWith(-0.1, expect.anything());
  });

  it('cannot be changed while disabled', async () => {
    const user = userEvent.setup();
    const { onChange } = setup({ disabled: true });
    const slider = screen.getByRole('slider');
    expect(slider).toHaveAttribute('data-disabled');
    slider.focus();
    await user.keyboard('{ArrowRight}');
    expect(onChange).not.toHaveBeenCalled();
  });

  describe('onReset', () => {
    it('is called by a double click on the thumb, the label or the value, and changes nothing itself', () => {
      const onReset = vi.fn();
      const { onChange } = setup({ onReset });
      fireEvent.doubleClick(screen.getByRole('slider'));
      fireEvent.doubleClick(screen.getByText('Black point, red'));
      fireEvent.doubleClick(screen.getByText('0.5'));
      expect(onReset).toHaveBeenCalledTimes(3);
      expect(onChange).not.toHaveBeenCalled();
    });

    it.each(['{Delete}', '{Backspace}'])('is called by %s on the focused thumb', async (key) => {
      const user = userEvent.setup();
      const onReset = vi.fn();
      const { onChange } = setup({ onReset });
      screen.getByRole('slider').focus();
      await user.keyboard(key);
      expect(onReset).toHaveBeenCalledTimes(1);
      expect(onChange).not.toHaveBeenCalled();
    });

    it('is not called by other keys', async () => {
      const user = userEvent.setup();
      const onReset = vi.fn();
      setup({ onReset });
      screen.getByRole('slider').focus();
      await user.keyboard('{ArrowRight}{Home}{Escape}');
      expect(onReset).not.toHaveBeenCalled();
    });

    it('is not called while disabled, and without it nothing happens', async () => {
      const user = userEvent.setup();
      const onReset = vi.fn();
      setup({ onReset, disabled: true });
      fireEvent.doubleClick(screen.getByRole('slider'));
      screen.getByRole('slider').focus();
      await user.keyboard('{Delete}');
      expect(onReset).not.toHaveBeenCalled();
    });

    it('does nothing without a handler', async () => {
      const user = userEvent.setup();
      const { onChange } = setup();
      fireEvent.doubleClick(screen.getByRole('slider'));
      screen.getByRole('slider').focus();
      await user.keyboard('{Delete}');
      expect(onChange).not.toHaveBeenCalled();
    });
  });

  describe('description', () => {
    it('is shown and is the accessible description of the slider', async () => {
      const { container } = setup({ description: 'Little headroom left.' });
      expect(screen.getByText('Little headroom left.')).toBeInTheDocument();
      expect(screen.getByRole('slider')).toHaveAccessibleDescription('Little headroom left.');
      await expectNoA11yViolations(container);
    });

    it.each([undefined, null, false, ''])('adds nothing for %j', async (description) => {
      const { container } = setup({ description });
      expect(screen.getByRole('slider')).not.toHaveAttribute('aria-describedby');
      expect(container.querySelector('p')).toBeNull();
      await expectNoA11yViolations(container);
    });
  });

  describe('swatch and trackGradient', () => {
    const GRADIENT = 'linear-gradient(to right, rgb(255 0 0), rgb(0 0 255))';

    it('shows a decorative swatch beside the label without changing the name', async () => {
      const { container } = setup({ swatch: 'rgb(255 0 0)' });
      const swatch = container.querySelector('[data-swatch]');
      expect(swatch).not.toBeNull();
      expect(swatch).toHaveAttribute('aria-hidden', 'true');
      expect(swatch?.getAttribute('style')).toContain('rgb(255 0 0)');
      expect(swatch).toHaveTextContent('');
      expect(screen.getByRole('slider', { name: 'Black point, red' })).toBeInTheDocument();
      expect(screen.getByText('Black point, red')).not.toContainElement(swatch as HTMLElement);
      await expectNoA11yViolations(container);
    });

    it('puts a gradient on the track, clears the range and marks the thumb', async () => {
      const { container } = setup({ trackGradient: GRADIENT });
      const marked = container.querySelectorAll('[data-gradient]');
      // The track, the filled range and the thumb.
      expect(marked).toHaveLength(3);
      expect(marked[0]?.getAttribute('style')).toContain(GRADIENT);
      expect(screen.getByRole('slider')).toHaveAttribute('data-gradient');
      expect(container.querySelector('[data-swatch]')).toBeNull();
      expect(screen.getByRole('slider', { name: 'Black point, red' })).toBeInTheDocument();
      await expectNoA11yViolations(container);
    });

    it('adds nothing without either', async () => {
      const { container } = setup();
      expect(container.querySelector('[data-swatch]')).toBeNull();
      expect(container.querySelector('[data-gradient]')).toBeNull();
      // Radix sets its own styles on the root and the thumb; the track has none of ours.
      // The thumb sits in a wrapper inside the root, after the track.
      const track = screen.getByRole('slider').parentElement?.parentElement?.firstElementChild;
      expect(track).toBeDefined();
      expect(track).not.toHaveAttribute('style');
      expect(container.innerHTML).not.toContain('--track-gradient');
      expect(container.innerHTML).not.toContain('--swatch');
      await expectNoA11yViolations(container);
    });
  });

  it('has no accessibility violations', async () => {
    const { container } = setup();
    await expectNoA11yViolations(container);
  });
});
