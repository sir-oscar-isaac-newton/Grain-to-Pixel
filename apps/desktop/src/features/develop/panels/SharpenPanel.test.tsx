import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../test-support';
import { makeRecipe } from '../../library/fake-ipc';
import { sharpenOf } from '../recipe-parts';
import { SharpenPanel } from './SharpenPanel';

type Props = Parameters<typeof SharpenPanel>[0];

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
  return { props, mocks, ...render(<SharpenPanel {...props} />) };
}

const slider = (name: string) => screen.getByRole('slider', { name });
const lastSharpen = (mocks: { onChange: ReturnType<typeof vi.fn> }) =>
  sharpenOf(mocks.onChange.mock.calls[mocks.onChange.mock.calls.length - 1]?.[0]);

describe('SharpenPanel', () => {
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
    expect(mocks.onChange.mock.calls[0]?.[1]).toEqual({ dragging: false });
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

  it('resets both values, and has nothing to reset at the defaults', async () => {
    const user = userEvent.setup();
    const { props, mocks, rerender } = setup();
    await user.click(screen.getByRole('button', { name: 'Reset sharpening' }));
    expect(lastSharpen(mocks)).toEqual({ amount: 0, radius_px: 1 });
    expect(mocks.onChange.mock.calls[0]?.[1]).toEqual({ dragging: false });
    rerender(<SharpenPanel {...props} recipe={makeRecipe('x')} />);
    expect(screen.getByRole('button', { name: 'Reset sharpening' })).toBeDisabled();
    // A radius that is not the default is something to reset, even at amount 0.
    rerender(
      <SharpenPanel
        {...props}
        recipe={makeRecipe('x', { output: { sharpen: { amount: 0, radius_px: 2 } } })}
      />,
    );
    expect(screen.getByRole('button', { name: 'Reset sharpening' })).toBeEnabled();
  });

  describe('when nothing can be changed', () => {
    it.each([
      ['a read-only library', { readOnly: true }],
      ['something being done', { working: true }],
    ])('disables every control for %s', (_name, over) => {
      setup(over);
      expect(screen.getByRole('button', { name: 'Reset sharpening' })).toBeDisabled();
      for (const s of screen.getAllByRole('slider')) expect(s).toHaveAttribute('data-disabled');
    });
  });

  it('has no accessibility violations', async () => {
    const { container } = setup();
    await expectNoA11yViolations(container);
  });
});
