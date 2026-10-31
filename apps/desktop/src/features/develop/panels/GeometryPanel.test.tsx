import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../test-support';
import { makeRecipe } from '../../library/fake-ipc';
import { geometryOf } from '../recipe-parts';
import { GeometryPanel } from './GeometryPanel';

type Props = Parameters<typeof GeometryPanel>[0];

const detected = { rect: { x: 0.05, y: 0.1, w: 0.9, h: 0.8 }, confidence: 0.9 };

function setup(over: Partial<Props> = {}) {
  const mocks = { onCroppingChange: vi.fn(), onGuideChange: vi.fn(), onChange: vi.fn() };
  const props: Props = {
    recipe: makeRecipe('x'),
    readOnly: false,
    working: false,
    cropping: false,
    cropReady: false,
    detected,
    ...mocks,
    ...over,
  };
  return { props, mocks, ...render(<GeometryPanel {...props} />) };
}

const whole = makeRecipe('x', {
  geometry: {
    crop: { x: 0, y: 0, w: 1, h: 1 },
    angle_deg: 0,
    rot90: 0,
    flip_h: false,
    flip_v: false,
  },
});
const slider = (name: string) => screen.getByRole('slider', { name });
const button = (name: string) => screen.getByRole('button', { name });
const lastGeometry = (mocks: { onChange: ReturnType<typeof vi.fn> }) =>
  geometryOf(mocks.onChange.mock.calls[mocks.onChange.mock.calls.length - 1]?.[0]);

describe('GeometryPanel', () => {
  describe('turning and flipping', () => {
    it('turns a quarter either way, round the four positions', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: makeRecipe('x', { geometry: { rot90: 0 } }) });
      await user.click(button('Rotate right'));
      expect(lastGeometry(mocks).rot90).toBe(1);
      expect(mocks.onChange.mock.calls[0]?.[1]).toEqual({ dragging: false });
      await user.click(button('Rotate left'));
      expect(lastGeometry(mocks).rot90).toBe(3);
    });

    it('goes from the last position round to the first', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: makeRecipe('x', { geometry: { rot90: 3 } }) });
      await user.click(button('Rotate right'));
      expect(lastGeometry(mocks).rot90).toBe(0);
    });

    it('flips each way and shows which flips are on', async () => {
      const user = userEvent.setup();
      const { mocks } = setup();
      expect(button('Flip horizontally')).toHaveAttribute('aria-pressed', 'true');
      expect(button('Flip vertically')).toHaveAttribute('aria-pressed', 'false');
      await user.click(button('Flip horizontally'));
      expect(lastGeometry(mocks).flip_h).toBe(false);
      await user.click(button('Flip vertically'));
      expect(lastGeometry(mocks).flip_v).toBe(true);
    });

    it('keeps the rest of the geometry when one thing changes', async () => {
      const user = userEvent.setup();
      const { mocks } = setup();
      await user.click(button('Rotate right'));
      expect(lastGeometry(mocks)).toMatchObject({
        crop: { x: 0.1, y: 0.1, w: 0.8, h: 0.8 },
        angle_deg: 1.5,
        flip_h: true,
      });
    });

    it('straightens from -45 to 45 degrees in steps of a tenth', async () => {
      const user = userEvent.setup();
      const { mocks } = setup();
      expect(slider('Straighten')).toHaveAttribute('aria-valuemin', '-45');
      expect(slider('Straighten')).toHaveAttribute('aria-valuemax', '45');
      expect(slider('Straighten')).toHaveAttribute('aria-valuetext', '1.5°');
      slider('Straighten').focus();
      await user.keyboard('{ArrowRight}');
      expect(lastGeometry(mocks).angle_deg).toBeCloseTo(1.6, 9);
      await user.keyboard('{PageUp}');
      // The mock does not feed the value back, so a page is ten steps from the original 1.5.
      expect(lastGeometry(mocks).angle_deg).toBeCloseTo(2.5, 9);
    });

    it('tells when the straighten slider is in use, for the grid', async () => {
      const user = userEvent.setup();
      const { mocks } = setup();
      slider('Straighten').focus();
      expect(mocks.onGuideChange).toHaveBeenLastCalledWith(true);
      await user.tab();
      expect(mocks.onGuideChange).toHaveBeenLastCalledWith(false);
    });

    it('resets the turns, flips and angle but not the crop', async () => {
      const user = userEvent.setup();
      const { mocks } = setup();
      await user.click(button('Reset turns and straighten'));
      expect(lastGeometry(mocks)).toEqual({
        crop: { x: 0.1, y: 0.1, w: 0.8, h: 0.8 },
        angle_deg: 0,
        rot90: 0,
        flip_h: false,
        flip_v: false,
      });
    });

    it('has nothing to reset when nothing is turned', () => {
      setup({ recipe: whole });
      expect(button('Reset turns and straighten')).toBeDisabled();
    });
  });

  describe('the crop', () => {
    it('shows the four sides in percent, each free to move up to the opposite one', () => {
      setup();
      expect(slider('Left side')).toHaveAttribute('aria-valuetext', '10.0%');
      expect(slider('Right side')).toHaveAttribute('aria-valuetext', '90.0%');
      expect(slider('Top side')).toHaveAttribute('aria-valuetext', '10.0%');
      expect(slider('Bottom side')).toHaveAttribute('aria-valuetext', '90.0%');
      expect(Number(slider('Left side').getAttribute('aria-valuemax'))).toBeCloseTo(0.89, 9);
      expect(Number(slider('Right side').getAttribute('aria-valuemin'))).toBeCloseTo(0.11, 9);
      expect(slider('Left side')).toHaveAttribute('aria-valuemin', '0');
      expect(slider('Bottom side')).toHaveAttribute('aria-valuemax', '1');
    });

    it('moves one side with the keyboard and leaves the others', async () => {
      const user = userEvent.setup();
      const { mocks } = setup();
      slider('Left side').focus();
      await user.keyboard('{ArrowRight}');
      const crop = lastGeometry(mocks).crop;
      expect(crop.x).toBeCloseTo(0.101, 9);
      expect(crop.x + crop.w).toBeCloseTo(0.9, 9);
      expect(crop.y).toBeCloseTo(0.1, 9);
      expect(crop.y + crop.h).toBeCloseTo(0.9, 9);

      slider('Bottom side').focus();
      await user.keyboard('{ArrowLeft}');
      expect(lastGeometry(mocks).crop.y + lastGeometry(mocks).crop.h).toBeCloseTo(0.899, 9);
    });

    it('turns editing on the picture on and off, and says what to do once it is ready', async () => {
      const user = userEvent.setup();
      const { props, mocks, rerender } = setup();
      expect(button('Edit crop')).toHaveAttribute('aria-pressed', 'false');
      await user.click(button('Edit crop'));
      expect(mocks.onCroppingChange).toHaveBeenLastCalledWith(true);

      rerender(<GeometryPanel {...props} cropping />);
      expect(button('Edit crop')).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByText('Preparing the whole scan…')).toBeInTheDocument();
      rerender(<GeometryPanel {...props} cropping cropReady />);
      expect(screen.getByText(/Drag the frame or its sides/)).toBeInTheDocument();
      await user.click(button('Edit crop'));
      expect(mocks.onCroppingChange).toHaveBeenLastCalledWith(false);
    });

    it('goes back to the detected frame, once it differs from the crop', async () => {
      const user = userEvent.setup();
      const { props, mocks, rerender } = setup();
      await user.click(button('Use the detected frame'));
      expect(lastGeometry(mocks).crop).toEqual(detected.rect);

      rerender(
        <GeometryPanel
          {...props}
          recipe={makeRecipe('x', { geometry: { crop: detected.rect } })}
        />,
      );
      expect(button('Use the detected frame')).toBeDisabled();
    });

    it('uses the whole scan, unless it already does', async () => {
      const user = userEvent.setup();
      const { props, mocks, rerender } = setup();
      await user.click(button('Use the whole scan'));
      expect(lastGeometry(mocks).crop).toEqual({ x: 0, y: 0, w: 1, h: 1 });
      rerender(<GeometryPanel {...props} recipe={whole} />);
      expect(button('Use the whole scan')).toBeDisabled();
    });

    it('says when the frame could not be found reliably, and offers no detected frame', () => {
      setup({ detected: { rect: { x: 0, y: 0, w: 1, h: 1 }, confidence: 0.3 } });
      expect(screen.getByRole('note')).toHaveTextContent(
        'The edges of the frame could not be found reliably, so the whole scan is used.',
      );
      expect(button('Use the detected frame')).toBeDisabled();
    });

    it('says nothing about detection before the analysis is done, or when it was confident', () => {
      const { rerender, props } = setup({ detected: null });
      expect(screen.queryByRole('note')).not.toBeInTheDocument();
      expect(button('Use the detected frame')).toBeDisabled();
      rerender(<GeometryPanel {...props} detected={detected} />);
      expect(screen.queryByRole('note')).not.toBeInTheDocument();
    });
  });

  describe('when nothing can be changed', () => {
    it.each([
      ['a read-only library', { readOnly: true }],
      ['something being done', { working: true }],
    ])('disables every control for %s', (_name, over) => {
      setup(over);
      for (const b of screen.getAllByRole('button')) expect(b).toBeDisabled();
      for (const s of screen.getAllByRole('slider')) expect(s).toHaveAttribute('data-disabled');
    });
  });

  it('has no accessibility violations, also while cropping and with the hint', async () => {
    const { container, props, rerender } = setup();
    await expectNoA11yViolations(container);
    rerender(
      <GeometryPanel
        {...props}
        cropping
        cropReady
        detected={{ rect: { x: 0, y: 0, w: 1, h: 1 }, confidence: 0.2 }}
      />,
    );
    await expectNoA11yViolations(container);
  });
});
