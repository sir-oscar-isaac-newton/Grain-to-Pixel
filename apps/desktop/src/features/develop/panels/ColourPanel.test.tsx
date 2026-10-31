import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../test-support';
import { makeRecipe } from '../../library/fake-ipc';
import { colourOf, monoOf } from '../recipe-parts';
import { ColourPanel } from './ColourPanel';

type Props = Parameters<typeof ColourPanel>[0];

function setup(over: Partial<Props> = {}) {
  const mocks = { onPickingChange: vi.fn(), onChange: vi.fn() };
  const props: Props = {
    recipe: makeRecipe('x', { color: { red_cyan: 12, saturation: 1.25 } }),
    readOnly: false,
    working: false,
    picking: false,
    pickReady: false,
    ...mocks,
    ...over,
  };
  return { props, mocks, ...render(<ColourPanel {...props} />) };
}

const slider = (name: string) => screen.getByRole('slider', { name });
const mono = makeRecipe('x', { mono: { enabled: true, mix: [0.25, 0.5, 0.25] } });

describe('ColourPanel', () => {
  it('shows the colour sliders with the recipe’s values', () => {
    setup();
    expect(slider('Cyan / red')).toHaveAttribute('aria-valuenow', '12');
    expect(slider('Cyan / red')).toHaveAttribute('aria-valuemin', '-100');
    expect(slider('Magenta / green')).toHaveAttribute('aria-valuenow', '0');
    expect(slider('Yellow / blue')).toHaveAttribute('aria-valuenow', '0');
    expect(slider('Temperature')).toHaveAttribute('aria-valuemax', '100');
    expect(slider('Tint')).toHaveAttribute('aria-valuenow', '0');
    expect(slider('Saturation')).toHaveAttribute('aria-valuenow', '1.25');
    expect(slider('Saturation')).toHaveAttribute('aria-valuemax', '2');
    expect(slider('Vibrance')).toHaveAttribute('aria-valuenow', '0');
  });

  it('moves a slider by one step with an arrow key and ten with Page Up', async () => {
    const user = userEvent.setup();
    const { mocks } = setup();
    slider('Temperature').focus();
    await user.keyboard('{ArrowRight}');
    const [first, options] = mocks.onChange.mock.calls[0] ?? [];
    expect(colourOf(first).temperature).toBe(1);
    expect(options).toEqual({ dragging: false });

    await user.keyboard('{PageUp}');
    const [second] = mocks.onChange.mock.calls[1] ?? [];
    expect(colourOf(second).temperature).toBe(10);
    // The rest of the colour block is as it was.
    expect(colourOf(second)).toMatchObject({ red_cyan: 12, saturation: 1.25 });
  });

  it('turns picking on and off with its button, which shows whether it is on', async () => {
    const user = userEvent.setup();
    const { props, mocks, rerender } = setup();
    expect(screen.getByRole('button', { name: 'Pick neutral grey' })).toHaveAttribute(
      'aria-pressed',
      'false',
    );
    await user.click(screen.getByRole('button', { name: 'Pick neutral grey' }));
    expect(mocks.onPickingChange).toHaveBeenLastCalledWith(true);

    rerender(<ColourPanel {...props} picking />);
    expect(screen.getByText('Preparing the picture…')).toBeInTheDocument();
    rerender(<ColourPanel {...props} picking pickReady />);
    expect(screen.getByText(/part of the picture that should be grey/)).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Pick neutral grey' }));
    expect(mocks.onPickingChange).toHaveBeenLastCalledWith(false);
  });

  describe('black and white', () => {
    it('is off by default, with no mix shown', () => {
      setup();
      expect(screen.getByRole('checkbox', { name: 'Black and white' })).not.toBeChecked();
      expect(screen.queryByRole('group', { name: 'Channel mix' })).not.toBeInTheDocument();
    });

    it('turns on with its switch, keeping the mix', async () => {
      const user = userEvent.setup();
      const { mocks } = setup();
      await user.click(screen.getByRole('checkbox', { name: 'Black and white' }));
      const [recipe, options] = mocks.onChange.mock.calls[0] ?? [];
      expect(monoOf(recipe).enabled).toBe(true);
      expect(monoOf(recipe).mix).toEqual([0.2627, 0.678, 0.0593]);
      expect(options).toEqual({ dragging: false });
    });

    it('shows the channel mix and hides saturation and vibrance, which it makes redundant', () => {
      setup({ recipe: mono });
      expect(screen.getByRole('checkbox', { name: 'Black and white' })).toBeChecked();
      const group = screen.getByRole('group', { name: 'Channel mix' });
      expect(group).toBeInTheDocument();
      expect(slider('Mix, red')).toHaveAttribute('aria-valuenow', '0.25');
      expect(slider('Mix, green')).toHaveAttribute('aria-valuenow', '0.5');
      expect(slider('Mix, blue')).toHaveAttribute('aria-valuemin', '-2');
      expect(screen.queryByRole('slider', { name: 'Saturation' })).not.toBeInTheDocument();
      expect(screen.queryByRole('slider', { name: 'Vibrance' })).not.toBeInTheDocument();
      // The axes, temperature and tint still act before the conversion.
      expect(slider('Tint')).toBeInTheDocument();
    });

    it('changes one weight at a time, and keeps the saturation for when it is turned off', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({
        recipe: makeRecipe('x', {
          color: { saturation: 1.5 },
          mono: { enabled: true, mix: [0.25, 0.5, 0.25] },
        }),
      });
      slider('Mix, green').focus();
      await user.keyboard('{ArrowRight}');
      const [recipe] = mocks.onChange.mock.calls[0] ?? [];
      expect(monoOf(recipe).mix).toEqual([0.25, 0.51, 0.25]);
      expect(colourOf(recipe).saturation).toBe(1.5);
    });
  });

  describe('when nothing can be changed', () => {
    it.each([
      ['a read-only library', { readOnly: true }],
      ['something being done', { working: true }],
    ])('disables every control for %s', (_name, over) => {
      setup({ recipe: mono, ...over });
      for (const button of screen.getAllByRole('button')) expect(button).toBeDisabled();
      for (const box of screen.getAllByRole('checkbox')) expect(box).toBeDisabled();
      for (const s of screen.getAllByRole('slider')) expect(s).toHaveAttribute('data-disabled');
    });
  });

  it('has no accessibility violations, also in black and white and while picking', async () => {
    const { container, props, rerender } = setup();
    await expectNoA11yViolations(container);
    rerender(<ColourPanel {...props} recipe={mono} picking pickReady />);
    await expectNoA11yViolations(container);
  });
});
