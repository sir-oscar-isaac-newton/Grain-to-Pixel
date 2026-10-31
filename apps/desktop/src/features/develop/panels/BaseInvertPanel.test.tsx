import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../test-support';
import { makeRecipe } from '../../library/fake-ipc';
import { baseOf, levelsOf } from '../recipe-parts';
import { BaseInvertPanel } from './BaseInvertPanel';

type Props = Parameters<typeof BaseInvertPanel>[0];

function setup(over: Partial<Props> = {}) {
  const mocks = {
    onPickChange: vi.fn(),
    onChange: vi.fn(),
    onAutoLevels: vi.fn(),
    onUseAsRollDefault: vi.fn(),
  };
  const props: Props = {
    recipe: makeRecipe('x'),
    rollBase: null,
    readOnly: false,
    pick: null,
    pickReady: false,
    working: false,
    ...mocks,
    ...over,
  };
  return { props, mocks, ...render(<BaseInvertPanel {...props} />) };
}

const manual = makeRecipe('x', { base: { source: 'manual', rgb: [0.8, 0.5, 0.3] } });

describe('BaseInvertPanel', () => {
  describe('the film base', () => {
    it('says it follows the roll, with the roll’s values', () => {
      setup({ rollBase: [0.9, 0.6, 0.4] });
      expect(screen.getByText('Following the roll’s film base.')).toBeInTheDocument();
      expect(screen.getByText('Red 0.900, green 0.600, blue 0.400')).toBeInTheDocument();
    });

    it('says when the roll has none yet, and shows the frame’s own', () => {
      setup({ rollBase: null });
      expect(screen.getByText(/The roll has no film base yet/)).toBeInTheDocument();
      expect(screen.getByText('Red 1.000, green 1.000, blue 1.000')).toBeInTheDocument();
    });

    it('says when it was picked on this frame, with the picked values', () => {
      setup({ recipe: manual, rollBase: [0.9, 0.6, 0.4] });
      expect(screen.getByText('Picked on this frame.')).toBeInTheDocument();
      expect(screen.getByText('Red 0.800, green 0.500, blue 0.300')).toBeInTheDocument();
    });

    it('turns picking on and off with its button, which shows whether it is on', async () => {
      const user = userEvent.setup();
      const { props, mocks, rerender } = setup();
      const pick = screen.getByRole('button', { name: 'Pick film base' });
      expect(pick).toHaveAttribute('aria-pressed', 'false');
      await user.click(pick);
      expect(mocks.onPickChange).toHaveBeenLastCalledWith('base');

      rerender(<BaseInvertPanel {...props} pick="base" pickReady />);
      expect(screen.getByRole('button', { name: 'Pick film base' })).toHaveAttribute(
        'aria-pressed',
        'true',
      );
      await user.click(screen.getByRole('button', { name: 'Pick film base' }));
      expect(mocks.onPickChange).toHaveBeenLastCalledWith(null);
    });

    it('tells what to do while picking, once the whole scan is shown', () => {
      const { props, rerender } = setup({ pick: 'base', pickReady: false });
      expect(screen.getByText('Preparing the whole scan…')).toBeInTheDocument();
      rerender(<BaseInvertPanel {...props} pick="base" pickReady />);
      expect(screen.getByText(/Click the clear film edge/)).toBeInTheDocument();
      expect(screen.queryByText('Preparing the whole scan…')).not.toBeInTheDocument();
    });

    it('goes back to the roll’s base, keeping the picked values, only when one was picked', async () => {
      const user = userEvent.setup();
      const { props, mocks, rerender } = setup();
      expect(screen.getByRole('button', { name: 'Use the roll’s base' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Use as the roll’s default' })).toBeDisabled();

      rerender(<BaseInvertPanel {...props} recipe={manual} />);
      await user.click(screen.getByRole('button', { name: 'Use the roll’s base' }));
      expect(mocks.onChange).toHaveBeenCalledTimes(1);
      const [recipe, options] = mocks.onChange.mock.calls[0] ?? [];
      expect(baseOf(recipe)).toEqual({ source: 'roll', rgb: [0.8, 0.5, 0.3] });
      expect(options).toEqual({ dragging: false });
    });

    it('makes the picked base the roll’s default', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: manual });
      await user.click(screen.getByRole('button', { name: 'Use as the roll’s default' }));
      expect(mocks.onUseAsRollDefault).toHaveBeenCalledTimes(1);
    });
  });

  describe('the levels', () => {
    it('has a slider for each channel of the black and the white point, at their values', () => {
      setup();
      const value = (name: string) =>
        screen.getByRole('slider', { name }).getAttribute('aria-valuenow');
      expect(value('Black point, red')).toBe('0.1');
      expect(value('Black point, green')).toBe('0.2');
      expect(value('Black point, blue')).toBe('0.3');
      expect(value('White point, red')).toBe('0.9');
      expect(value('White point, green')).toBe('0.8');
      expect(value('White point, blue')).toBe('0.7');
      expect(screen.getAllByRole('slider')).toHaveLength(6);
      expect(screen.getByRole('group', { name: 'Black point' })).toBeInTheDocument();
      expect(screen.getByRole('group', { name: 'White point' })).toBeInTheDocument();
    });

    it('changes one channel of one point and leaves the rest, as a release', async () => {
      const user = userEvent.setup();
      const { props, mocks } = setup();
      screen.getByRole('slider', { name: 'White point, green' }).focus();
      await user.keyboard('{ArrowRight}');
      expect(mocks.onChange).toHaveBeenCalledTimes(1);
      const [recipe, options] = mocks.onChange.mock.calls[0] ?? [];
      expect(levelsOf(recipe)).toEqual({ black: [0.1, 0.2, 0.3], white: [0.9, 0.805, 0.7] });
      expect(options).toEqual({ dragging: false });
      // The recipe that was passed in is not changed.
      expect(levelsOf(props.recipe).white[1]).toBe(0.8);
    });

    it('lets a slider run past 4 when the value is higher', () => {
      setup({ recipe: makeRecipe('x', { invert: { black: [0, 0, 0], white: [6.2, 1, 1] } }) });
      expect(screen.getByRole('slider', { name: 'White point, red' })).toHaveAttribute(
        'aria-valuemax',
        '7',
      );
      expect(screen.getByRole('slider', { name: 'White point, green' })).toHaveAttribute(
        'aria-valuemax',
        '4',
      );
    });

    it.each([
      ['white', 'Pick white point'],
      ['black', 'Pick black point'],
    ])('starts and stops picking the %s point with its button', async (point, name) => {
      const user = userEvent.setup();
      const { props, mocks, rerender } = setup();
      expect(screen.getByRole('button', { name })).toHaveAttribute('aria-pressed', 'false');
      await user.click(screen.getByRole('button', { name }));
      expect(mocks.onPickChange).toHaveBeenLastCalledWith(point);

      rerender(<BaseInvertPanel {...props} pick={point as 'white' | 'black'} pickReady />);
      expect(screen.getByRole('button', { name })).toHaveAttribute('aria-pressed', 'true');
      await user.click(screen.getByRole('button', { name }));
      expect(mocks.onPickChange).toHaveBeenLastCalledWith(null);
    });

    it('tells what to pick while picking a level, once the picture is ready', () => {
      const { props, rerender } = setup({ pick: 'white', pickReady: false });
      expect(screen.getByText('Preparing the picture…')).toBeInTheDocument();
      rerender(<BaseInvertPanel {...props} pick="white" pickReady />);
      expect(screen.getByText(/brightest part of the picture/)).toBeInTheDocument();
      rerender(<BaseInvertPanel {...props} pick="black" pickReady />);
      expect(screen.getByText(/darkest part of the picture/)).toBeInTheDocument();
      expect(screen.queryByText(/clear film edge/)).not.toBeInTheDocument();
    });

    it('re-runs the automatic levels', async () => {
      const user = userEvent.setup();
      const { mocks } = setup();
      await user.click(screen.getByRole('button', { name: 'Re-run auto levels' }));
      expect(mocks.onAutoLevels).toHaveBeenCalledTimes(1);
    });
  });

  describe('when nothing can be changed', () => {
    it.each([
      ['a read-only library', { readOnly: true }],
      ['something being done', { working: true }],
    ])('disables every control for %s', (_name, over) => {
      setup({ recipe: manual, ...over });
      for (const button of screen.getAllByRole('button')) expect(button).toBeDisabled();
      for (const slider of screen.getAllByRole('slider')) {
        expect(slider).toHaveAttribute('data-disabled');
      }
    });
  });

  it('has no accessibility violations, also while picking', async () => {
    const { container, props, rerender } = setup({ recipe: manual, rollBase: [0.9, 0.6, 0.4] });
    await expectNoA11yViolations(container);
    rerender(<BaseInvertPanel {...props} pick="base" pickReady />);
    await expectNoA11yViolations(container);
    rerender(<BaseInvertPanel {...props} pick="white" pickReady />);
    await expectNoA11yViolations(container);
  });
});
