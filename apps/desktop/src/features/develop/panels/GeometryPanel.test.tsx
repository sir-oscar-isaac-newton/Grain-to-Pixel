import { fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../test-support';
import { makeRecipe } from '../../library/fake-ipc';
import { FREE_RATIO, ratioOfCrop, type RatioState, type ScanSize } from '../crop-ratio';
import { geometryOf } from '../recipe-parts';
import { GeometryPanel } from './GeometryPanel';

type Props = Parameters<typeof GeometryPanel>[0];

/** A 6000 x 4000 scan: 3:2, landscape. */
const scan: ScanSize = { width: 6000, height: 4000 };
const detected = { rect: { x: 0.05, y: 0.1, w: 0.9, h: 0.8 }, confidence: 0.9 };

function setup(over: Partial<Props> = {}) {
  const mocks = {
    onCroppingChange: vi.fn(),
    onGuideChange: vi.fn(),
    onRatioChange: vi.fn(),
    onChange: vi.fn(),
  };
  const props: Props = {
    recipe: makeRecipe('x'),
    readOnly: false,
    working: false,
    cropping: false,
    cropReady: false,
    detected,
    ratio: FREE_RATIO,
    scan,
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

  describe('the aspect ratio', () => {
    const locked = (mode: RatioState['mode'], over: Partial<RatioState> = {}): RatioState => ({
      ...FREE_RATIO,
      mode,
      ...over,
    });
    const ratioSelect = () => screen.getByRole('combobox', { name: 'Aspect ratio' });
    const lastCrop = (mocks: { onChange: ReturnType<typeof vi.fn> }) => lastGeometry(mocks).crop;

    it('offers free, the scan’s own ratio, the five presets and a custom one, starting free', () => {
      setup();
      expect(ratioSelect()).toHaveValue('free');
      expect(
        within(ratioSelect())
          .getAllByRole('option')
          .map((o) => o.textContent),
      ).toEqual(['Free', 'Same as the scan', '3:2', '4:3', '5:4', '1:1', '16:9', 'Custom…']);
      // Nothing to swap or fit while the crop is free.
      expect(screen.queryByRole('button', { name: 'Swap landscape and portrait' })).toBeNull();
      expect(screen.queryByText(/keeps the ratio/)).toBeNull();
    });

    it('needs the size of the scan', () => {
      setup({ scan: null });
      expect(ratioSelect()).toBeDisabled();
    });

    it('fits the largest crop of a preset around the centre of the crop when it is chosen', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: whole });
      await user.selectOptions(ratioSelect(), '1:1');
      expect(mocks.onRatioChange).toHaveBeenLastCalledWith(locked('1:1'));
      // 4000 x 4000 of 6000 x 4000: two thirds of the width, all of the height, centred.
      const crop = lastCrop(mocks);
      expect(crop.h).toBe(1);
      expect(crop.w).toBeCloseTo(2 / 3, 9);
      expect(crop.x).toBeCloseTo(1 / 6, 9);
      expect(ratioOfCrop(crop, scan, 0)).toBeCloseTo(1, 9);
    });

    it('leaves the crop alone when it already has the ratio', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: whole });
      await user.selectOptions(ratioSelect(), '3:2');
      expect(mocks.onRatioChange).toHaveBeenCalledTimes(1);
      expect(mocks.onChange).not.toHaveBeenCalled();
    });

    it('goes back to no lock without moving the crop', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: whole, ratio: locked('1:1') });
      await user.selectOptions(ratioSelect(), 'free');
      expect(mocks.onRatioChange).toHaveBeenLastCalledWith(locked('free'));
      expect(mocks.onChange).not.toHaveBeenCalled();
    });

    it('says what the ratio is while it is locked, for a preset, a portrait one and a custom one', () => {
      const { rerender, props } = setup({ recipe: whole, ratio: locked('4:3') });
      expect(screen.getByText(/keeps the ratio 4:3 \(width to height\)/)).toBeInTheDocument();
      rerender(
        <GeometryPanel {...props} recipe={whole} ratio={locked('4:3', { portrait: true })} />,
      );
      expect(screen.getByText(/keeps the ratio 3:4/)).toBeInTheDocument();
      rerender(
        <GeometryPanel
          {...props}
          recipe={whole}
          ratio={locked('custom', { custom: { w: '1.85', h: '1' } })}
        />,
      );
      expect(screen.getByText(/keeps the ratio 1\.85 to 1/)).toBeInTheDocument();
      rerender(<GeometryPanel {...props} recipe={whole} ratio={locked('scan')} />);
      expect(screen.getByText(/keeps the ratio 1\.50 to 1/)).toBeInTheDocument();
    });

    it('keeps the ratio when a side is set with its slider', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: whole, ratio: locked('3:2') });
      slider('Left side').focus();
      await user.keyboard('{ArrowRight}');
      const crop = lastCrop(mocks);
      expect(crop.x).toBeCloseTo(0.001, 9);
      expect(crop.w).toBeCloseTo(0.999, 9);
      expect(ratioOfCrop(crop, scan, 0)).toBeCloseTo(1.5, 9);
      slider('Bottom side').focus();
      await user.keyboard('{ArrowLeft}');
      expect(ratioOfCrop(lastCrop(mocks), scan, 0)).toBeCloseTo(1.5, 9);
    });

    it('swaps landscape and portrait, refitting the crop', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: whole, ratio: locked('3:2') });
      await user.click(button('Swap landscape and portrait'));
      expect(mocks.onRatioChange).toHaveBeenLastCalledWith(locked('3:2', { portrait: true }));
      const crop = lastCrop(mocks);
      expect(ratioOfCrop(crop, scan, 0)).toBeCloseTo(2 / 3, 9);
      expect(crop.h).toBe(1);
    });

    it('fits the largest crop again after the crop was changed another way', async () => {
      const user = userEvent.setup();
      const off = makeRecipe('x', {
        geometry: { crop: { x: 0.1, y: 0.1, w: 0.5, h: 0.5 }, angle_deg: 0, rot90: 0 },
      });
      const { props, mocks, rerender } = setup({ recipe: off, ratio: locked('3:2') });
      await user.click(button('Fit the largest crop'));
      expect(ratioOfCrop(lastCrop(mocks), scan, 0)).toBeCloseTo(1.5, 9);
      // Fitted already, so there is nothing more to do.
      rerender(<GeometryPanel {...props} recipe={whole} ratio={locked('3:2')} />);
      expect(button('Fit the largest crop')).toBeDisabled();
    });

    it('refits the crop when the picture is turned, so the finished picture keeps its ratio', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: whole, ratio: locked('3:2') });
      await user.click(button('Rotate right'));
      const geometry = lastGeometry(mocks);
      expect(geometry.rot90).toBe(1);
      // Turned a quarter, 3:2 is 2:3 in the scan: 2667 x 4000 pixels.
      expect(geometry.crop.h).toBe(1);
      expect(geometry.crop.w).toBeCloseTo(4 / 9, 9);
      expect(ratioOfCrop(geometry.crop, scan, 1)).toBeCloseTo(1.5, 9);

      await user.click(button('Rotate left'));
      expect(lastGeometry(mocks).rot90).toBe(3);
      expect(ratioOfCrop(lastGeometry(mocks).crop, scan, 3)).toBeCloseTo(1.5, 9);
    });

    it('does not move the crop when the picture is turned with no lock', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: whole });
      await user.click(button('Rotate right'));
      expect(lastCrop(mocks)).toEqual({ x: 0, y: 0, w: 1, h: 1 });
    });

    it('lets the scan’s own ratio turn with the picture', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: whole, ratio: locked('scan') });
      await user.click(button('Rotate right'));
      // The scan's own shape is the whole scan, whichever way it is turned.
      expect(lastCrop(mocks)).toEqual({ x: 0, y: 0, w: 1, h: 1 });
    });

    it('takes the largest crop of the ratio inside the detected frame and inside the scan', async () => {
      const user = userEvent.setup();
      const off = makeRecipe('x', {
        geometry: { crop: { x: 0.3, y: 0.3, w: 0.2, h: 0.2 }, angle_deg: 0, rot90: 0 },
      });
      const { mocks } = setup({ recipe: off, ratio: locked('3:2') });
      await user.click(button('Use the detected frame'));
      // The detected frame is 0.9 x 0.8 of the scan: 3:2 is square in fractions, so 0.8 x 0.8.
      let crop = lastCrop(mocks);
      expect(crop.w).toBeCloseTo(0.8, 9);
      expect(crop.h).toBeCloseTo(0.8, 9);
      expect(crop.x).toBeCloseTo(0.1, 9);
      expect(crop.y).toBeCloseTo(0.1, 9);
      await user.click(button('Use the whole scan'));
      crop = lastCrop(mocks);
      expect(ratioOfCrop(crop, scan, 0)).toBeCloseTo(1.5, 9);
      expect(crop.w).toBeCloseTo(1, 9);
    });

    it('takes a custom ratio as typed, and refits as it is typed', () => {
      const custom = locked('custom', { custom: { w: '2', h: '1' } });
      const { mocks } = setup({ recipe: whole, ratio: custom });
      const width = screen.getByRole('textbox', { name: 'Ratio width' });
      const height = screen.getByRole('textbox', { name: 'Ratio height' });
      expect(width).toHaveValue('2');
      expect(height).toHaveValue('1');
      fireEvent.change(height, { target: { value: '1.5' } });
      expect(mocks.onRatioChange).toHaveBeenLastCalledWith(
        locked('custom', { custom: { w: '2', h: '1.5' } }),
      );
      expect(ratioOfCrop(lastCrop(mocks), scan, 0)).toBeCloseTo(2 / 1.5, 9);
    });

    it('says when the custom ratio cannot be used, and changes nothing', () => {
      const { mocks } = setup({
        recipe: whole,
        ratio: locked('custom', { custom: { w: '0', h: '1' } }),
      });
      expect(screen.getByRole('alert')).toHaveTextContent('Use positive numbers');
      expect(screen.getByRole('textbox', { name: 'Ratio width' })).toHaveAttribute(
        'aria-invalid',
        'true',
      );
      expect(button('Swap landscape and portrait')).toBeDisabled();
      expect(button('Fit the largest crop')).toBeDisabled();
      fireEvent.change(screen.getByRole('textbox', { name: 'Ratio height' }), {
        target: { value: '' },
      });
      expect(mocks.onChange).not.toHaveBeenCalled();
    });

    it('is out of reach when nothing can be changed', () => {
      setup({ recipe: whole, ratio: locked('custom'), readOnly: true });
      expect(ratioSelect()).toBeDisabled();
      expect(screen.getByRole('textbox', { name: 'Ratio width' })).toBeDisabled();
      expect(button('Swap landscape and portrait')).toBeDisabled();
    });

    it('has no accessibility violations, free, locked and custom', async () => {
      const { container, props, rerender } = setup({ recipe: whole });
      await expectNoA11yViolations(container);
      rerender(<GeometryPanel {...props} recipe={whole} ratio={locked('16:9')} />);
      await expectNoA11yViolations(container);
      rerender(<GeometryPanel {...props} recipe={whole} ratio={locked('custom')} />);
      await expectNoA11yViolations(container);
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
