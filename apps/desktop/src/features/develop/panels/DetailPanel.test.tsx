import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../test-support';
import { makeRecipe } from '../../library/fake-ipc';
import { noiseOf, sharpenOf } from '../recipe-parts';
import { DetailPanel } from './DetailPanel';

type Props = Parameters<typeof DetailPanel>[0];

const sharp = makeRecipe('x', {
  output: { space: 'srgb', sharpen: { amount: 60, radius_px: 1.5 } },
});

function setup(over: Partial<Props> = {}) {
  const mocks = { onChange: vi.fn() };
  const props: Props = {
    recipe: sharp,
    readOnly: false,
    working: false,
    ...mocks,
    ...over,
  };
  return { props, mocks, ...render(<DetailPanel {...props} />) };
}

const slider = (name: string) => screen.getByRole('slider', { name });
const lastCall = (mocks: { onChange: ReturnType<typeof vi.fn> }) =>
  mocks.onChange.mock.calls[mocks.onChange.mock.calls.length - 1];
const lastSharpen = (mocks: { onChange: ReturnType<typeof vi.fn> }) =>
  sharpenOf(lastCall(mocks)?.[0]);
const lastNoise = (mocks: { onChange: ReturnType<typeof vi.fn> }) => noiseOf(lastCall(mocks)?.[0]);

const noisy = makeRecipe('x', {
  noise: { luminance: 40, color: 70 },
  output: { space: 'srgb', sharpen: { amount: 60, radius_px: 1.5 } },
});

describe('DetailPanel', () => {
  it('is called Detail and has a section for the noise reduction and one for the sharpening', () => {
    setup();
    expect(screen.getByRole('heading', { name: 'Detail' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Noise reduction' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Output sharpening' })).toBeInTheDocument();
    expect(screen.getByRole('group', { name: 'Noise reduction' })).toBeInTheDocument();
  });

  it('shows the luminance and the colour amount, from 0 to 100', () => {
    setup({ recipe: noisy });
    expect(slider('Luminance')).toHaveAttribute('aria-valuenow', '40');
    expect(slider('Luminance')).toHaveAttribute('aria-valuemin', '0');
    expect(slider('Luminance')).toHaveAttribute('aria-valuemax', '100');
    expect(slider('Colour')).toHaveAttribute('aria-valuenow', '70');
    expect(slider('Colour')).toHaveAttribute('aria-valuemax', '100');
  });

  it('starts off for a recipe with no noise block', () => {
    setup({ recipe: makeRecipe('x') });
    expect(slider('Luminance')).toHaveAttribute('aria-valuenow', '0');
    expect(slider('Colour')).toHaveAttribute('aria-valuenow', '0');
  });

  it('says that fine grain only shows in the exported file', () => {
    setup();
    const note = screen.getByText(/Fine grain only shows in the exported file/);
    expect(note).toHaveTextContent('the preview is smaller');
    const group = screen.getByRole('group', { name: 'Noise reduction' });
    expect(group).toHaveAccessibleDescription(note.textContent ?? '');
  });

  it('changes the luminance and the colour amount, one step with an arrow key', async () => {
    const user = userEvent.setup();
    const { mocks } = setup({ recipe: noisy });
    slider('Luminance').focus();
    await user.keyboard('{ArrowRight}');
    expect(lastNoise(mocks)).toEqual({ luminance: 41, color: 70 });
    // The picture on screen shows it, so the preview is asked for (no preview: false).
    expect(lastCall(mocks)?.[1]).toEqual({ dragging: false });
    slider('Colour').focus();
    await user.keyboard('{ArrowLeft}');
    expect(lastNoise(mocks)).toEqual({ luminance: 40, color: 69 });
    await user.keyboard('{End}');
    expect(lastNoise(mocks).color).toBe(100);
    await user.keyboard('{Home}');
    expect(lastNoise(mocks).color).toBe(0);
    // The sharpening stays as it was.
    expect(lastSharpen(mocks)).toEqual({ amount: 60, radius_px: 1.5 });
  });

  it('shows the amount and the radius with their units and ranges', () => {
    setup();
    expect(slider('Amount')).toHaveAttribute('aria-valuenow', '60');
    expect(slider('Amount')).toHaveAttribute('aria-valuemin', '0');
    expect(slider('Amount')).toHaveAttribute('aria-valuemax', '150');
    expect(slider('Amount')).toHaveAttribute('aria-valuetext', '60%');
    expect(slider('Radius')).toHaveAttribute('aria-valuenow', '1.5');
    expect(slider('Radius')).toHaveAttribute('aria-valuemin', '0.3');
    expect(slider('Radius')).toHaveAttribute('aria-valuemax', '3');
    expect(slider('Radius')).toHaveAttribute('aria-valuetext', '1.5 px');
  });

  it('starts off, at a radius of one pixel, for a recipe with no sharpening', () => {
    setup({ recipe: makeRecipe('x') });
    expect(slider('Amount')).toHaveAttribute('aria-valuenow', '0');
    expect(slider('Radius')).toHaveAttribute('aria-valuetext', '1.0 px');
  });

  it('says the preview is not sharpened and what the radius is measured in', () => {
    setup();
    const note = screen.getByText(/Applied when you export, not in the preview/);
    expect(note).toHaveTextContent('pixels of the exported image');
    const group = screen.getByRole('group', { name: 'Output sharpening' });
    expect(group).toHaveAccessibleDescription(note.textContent ?? '');
  });

  it('moves the amount by one step with an arrow key and ten with Page Up', async () => {
    const user = userEvent.setup();
    const { mocks } = setup();
    slider('Amount').focus();
    await user.keyboard('{ArrowRight}');
    expect(lastSharpen(mocks)).toEqual({ amount: 61, radius_px: 1.5 });
    expect(mocks.onChange.mock.calls[0]?.[1]).toEqual({ dragging: false, preview: false });
    await user.keyboard('{PageUp}');
    expect(lastSharpen(mocks).amount).toBe(70);
    await user.keyboard('{End}');
    expect(lastSharpen(mocks).amount).toBe(150);
  });

  it('moves the radius by a tenth, without the rounding noise of floating point', async () => {
    const user = userEvent.setup();
    const { mocks } = setup({
      recipe: makeRecipe('x', { output: { sharpen: { amount: 20, radius_px: 0.9 } } }),
    });
    slider('Radius').focus();
    await user.keyboard('{ArrowRight}');
    expect(lastSharpen(mocks).radius_px).toBe(1);
    await user.keyboard('{Home}');
    expect(lastSharpen(mocks).radius_px).toBe(0.3);
    await user.keyboard('{End}');
    expect(lastSharpen(mocks).radius_px).toBe(3);
  });

  it('keeps the colour space and the rest of the recipe when it changes', async () => {
    const user = userEvent.setup();
    const { mocks } = setup({
      recipe: makeRecipe('x', {
        output: { space: 'adobe_rgb', sharpen: { amount: 10, radius_px: 1 } },
      }),
    });
    slider('Amount').focus();
    await user.keyboard('{ArrowRight}');
    const next = mocks.onChange.mock.calls[0]?.[0] as {
      output: { space: string };
      geometry: unknown;
    };
    expect(next.output.space).toBe('adobe_rgb');
    expect(next.geometry).toBeDefined();
  });

  it('has one reset, the header one, which puts the noise reduction and the sharpening back', async () => {
    const user = userEvent.setup();
    const auto = makeRecipe('auto', {
      noise: { luminance: 5, color: 10 },
      output: { space: 'srgb', sharpen: { amount: 25, radius_px: 0.8 } },
    });
    const { mocks } = setup({ recipe: noisy, auto });
    expect(screen.getAllByRole('button', { name: /^Reset/ })).toHaveLength(1);
    await user.click(screen.getByRole('button', { name: 'Reset Detail to automatic' }));
    expect(lastNoise(mocks)).toEqual({ luminance: 5, color: 10 });
    expect(lastSharpen(mocks)).toEqual({ amount: 25, radius_px: 0.8 });
    expect(mocks.onChange.mock.calls[0]?.[1]).toEqual({ dragging: false });
  });

  it('puts the noise reduction to zero when the automatic recipe has none', async () => {
    const user = userEvent.setup();
    const { mocks } = setup({ recipe: noisy, auto: makeRecipe('auto') });
    await user.click(screen.getByRole('button', { name: 'Reset Detail to automatic' }));
    expect(lastNoise(mocks)).toEqual({ luminance: 0, color: 0 });
  });

  it('marks a slider reset as one to merge with a change just made', () => {
    const { mocks } = setup({ recipe: noisy, auto: makeRecipe('auto') });
    fireEvent.doubleClick(slider('Amount'));
    expect(mocks.onChange.mock.calls[0]?.[1]).toEqual({
      dragging: false,
      merge: true,
      preview: false,
    });
  });

  it('resets each noise slider on its own, to the automatic value', () => {
    const auto = makeRecipe('auto', { noise: { luminance: 12, color: 34 } });
    const { mocks } = setup({ recipe: noisy, auto });
    fireEvent.doubleClick(slider('Luminance'));
    expect(lastNoise(mocks)).toEqual({ luminance: 12, color: 70 });
    expect(lastCall(mocks)?.[1]).toEqual({ dragging: false, merge: true });
    fireEvent.doubleClick(slider('Colour'));
    expect(lastNoise(mocks)).toEqual({ luminance: 40, color: 34 });
    expect(lastCall(mocks)?.[1]).toEqual({ dragging: false, merge: true });
  });

  describe('when nothing can be changed', () => {
    it.each([
      ['a read-only library', { readOnly: true }],
      ['something being done', { working: true }],
    ])('disables every control for %s', (_name, over) => {
      setup(over);
      expect(screen.getByRole('button', { name: 'Reset Detail to automatic' })).toBeDisabled();
      for (const s of screen.getAllByRole('slider')) expect(s).toHaveAttribute('data-disabled');
    });
  });

  it('has no accessibility violations', async () => {
    const { container } = setup();
    await expectNoA11yViolations(container);
  });
});
