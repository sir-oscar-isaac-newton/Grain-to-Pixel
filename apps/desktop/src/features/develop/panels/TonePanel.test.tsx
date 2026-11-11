import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../test-support';
import { makeRecipe } from '../../library/fake-ipc';
import { curvesOf, toneOf } from '../recipe-parts';
import { TonePanel } from './TonePanel';

type Props = Parameters<typeof TonePanel>[0];

function setup(over: Partial<Props> = {}) {
  const mocks = { onChange: vi.fn(), onApplyStartingLook: vi.fn() };
  const props: Props = {
    recipe: makeRecipe('x', { tone: { exposure_ev: 0.5, contrast: -20 } }),
    readOnly: false,
    working: false,
    ...mocks,
    ...over,
  };
  return { props, mocks, ...render(<TonePanel {...props} />) };
}

const slider = (name: string) => screen.getByRole('slider', { name });

describe('TonePanel', () => {
  it('shows the tone sliders with the recipe’s values and ranges', () => {
    setup();
    expect(slider('Exposure')).toHaveAttribute('aria-valuenow', '0.5');
    expect(slider('Exposure')).toHaveAttribute('aria-valuemin', '-5');
    expect(slider('Exposure')).toHaveAttribute('aria-valuemax', '5');
    expect(slider('Exposure')).toHaveAttribute('aria-valuetext', '0.50 EV');
    expect(slider('Contrast')).toHaveAttribute('aria-valuenow', '-20');
    for (const name of ['Highlights', 'Shadows', 'Whites', 'Blacks', 'Dehaze']) {
      expect(slider(name)).toHaveAttribute('aria-valuenow', '0');
      expect(slider(name)).toHaveAttribute('aria-valuemax', '100');
    }
  });

  it('has the two vignette sliders, with their ranges, below the others', () => {
    setup({
      recipe: makeRecipe('x', { tone: { vignette: { amount: -35, midpoint: 60 } } }),
    });
    expect(slider('Vignette')).toHaveAttribute('aria-valuenow', '-35');
    expect(slider('Vignette')).toHaveAttribute('aria-valuemin', '-100');
    expect(slider('Vignette')).toHaveAttribute('aria-valuemax', '100');
    expect(slider('Vignette midpoint')).toHaveAttribute('aria-valuenow', '60');
    expect(slider('Vignette midpoint')).toHaveAttribute('aria-valuemin', '0');
    expect(slider('Vignette midpoint')).toHaveAttribute('aria-valuemax', '100');
    const follows = (a: HTMLElement, b: HTMLElement) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(follows(slider('Dehaze'), slider('Vignette'))).toBe(true);
    expect(follows(slider('Vignette'), slider('Vignette midpoint'))).toBe(true);
  });

  it('shows the vignette defaults for a recipe that has none', () => {
    setup({ recipe: makeRecipe('x') });
    expect(slider('Vignette')).toHaveAttribute('aria-valuenow', '0');
    expect(slider('Vignette midpoint')).toHaveAttribute('aria-valuenow', '50');
  });

  it('changes the vignette in the recipe, one step at a time, keeping the rest', async () => {
    const user = userEvent.setup();
    const { mocks } = setup({
      recipe: makeRecipe('x', { tone: { contrast: 4, vignette: { amount: 0, midpoint: 50 } } }),
    });
    slider('Vignette').focus();
    await user.keyboard('{ArrowLeft}');
    const [first, options] = mocks.onChange.mock.calls[0] ?? [];
    expect(first.tone.vignette).toEqual({ amount: -1, midpoint: 50 });
    expect(toneOf(first).contrast).toBe(4);
    expect(options).toEqual({ dragging: false });

    slider('Vignette midpoint').focus();
    await user.keyboard('{PageDown}');
    const [second] = mocks.onChange.mock.calls[1] ?? [];
    expect(second.tone.vignette).toEqual({ amount: 0, midpoint: 40 });
  });

  it('resets each vignette slider on a double click to the automatic one, merging', () => {
    const auto = makeRecipe('auto', { tone: { vignette: { amount: 0, midpoint: 50 } } });
    const { mocks } = setup({
      auto,
      recipe: makeRecipe('x', { tone: { vignette: { amount: -60, midpoint: 10 } } }),
    });
    fireEvent.doubleClick(slider('Vignette'));
    const [first, firstOptions] = mocks.onChange.mock.calls[0] ?? [];
    expect(first.tone.vignette).toEqual({ amount: 0, midpoint: 10 });
    expect(firstOptions).toEqual({ dragging: false, merge: true });
    fireEvent.doubleClick(slider('Vignette midpoint'));
    const [second, secondOptions] = mocks.onChange.mock.calls[1] ?? [];
    expect(second.tone.vignette).toEqual({ amount: -60, midpoint: 50 });
    expect(secondOptions).toEqual({ dragging: false, merge: true });
  });

  it('puts the vignette back to the automatic one with the panel reset', async () => {
    const user = userEvent.setup();
    const { mocks } = setup({
      auto: makeRecipe('auto', { tone: { vignette: { amount: 0, midpoint: 50 } } }),
      recipe: makeRecipe('x', { tone: { vignette: { amount: 45, midpoint: 5 } } }),
    });
    await user.click(screen.getByRole('button', { name: 'Reset Tone to automatic' }));
    const [recipe] = mocks.onChange.mock.calls[0] ?? [];
    expect(recipe.tone.vignette).toEqual({ amount: 0, midpoint: 50 });
  });

  it('moves exposure by 0.01 and the others by one step, ten with Page Up', async () => {
    const user = userEvent.setup();
    const { mocks } = setup();
    slider('Exposure').focus();
    await user.keyboard('{ArrowRight}');
    const [first, options] = mocks.onChange.mock.calls[0] ?? [];
    expect(toneOf(first).exposure_ev).toBeCloseTo(0.51, 9);
    expect(toneOf(first).contrast).toBe(-20);
    expect(options).toEqual({ dragging: false });

    slider('Shadows').focus();
    await user.keyboard('{PageUp}');
    const [second] = mocks.onChange.mock.calls[1] ?? [];
    expect(toneOf(second).shadows).toBe(10);
  });

  it('has one reset, the header one, which puts the sliders and curves to the automatic ones', async () => {
    const user = userEvent.setup();
    const auto = makeRecipe('auto', {
      tone: {
        exposure_ev: 0.2,
        contrast: 5,
        curve: {
          master: [
            [0, 0.05],
            [1, 1],
          ],
          r: [],
          g: [],
          b: [],
        },
      },
    });
    const { mocks } = setup({
      auto,
      recipe: makeRecipe('x', { tone: { exposure_ev: 1, whites: 30 } }),
    });
    expect(screen.getAllByRole('button', { name: /^Reset(?! curve)/ })).toHaveLength(1);
    await user.click(screen.getByRole('button', { name: 'Reset Tone to automatic' }));
    const [recipe, options] = mocks.onChange.mock.calls[0] ?? [];
    expect(toneOf(recipe)).toEqual(toneOf(auto));
    expect(curvesOf(recipe).master[0]).toEqual([0, 0.05]);
    expect(options).toEqual({ dragging: false });
  });

  it('marks a slider reset as one to merge with a change just made', async () => {
    const { mocks } = setup({ auto: makeRecipe('auto') });
    fireEvent.doubleClick(slider('Contrast'));
    expect(mocks.onChange.mock.calls[0]?.[1]).toEqual({ dragging: false, merge: true });
  });

  it('sends a changed curve with the rest of the tone block', async () => {
    const user = userEvent.setup();
    const { mocks } = setup({
      recipe: makeRecipe('x', {
        tone: {
          contrast: 7,
          curve: {
            master: [
              [0, 0],
              [1, 1],
            ],
            r: [],
            g: [],
            b: [],
          },
        },
      }),
    });
    await user.click(screen.getByRole('button', { name: 'Add point' }));
    const [recipe] = mocks.onChange.mock.calls[0] ?? [];
    expect(curvesOf(recipe).master).toHaveLength(3);
    expect(toneOf(recipe).contrast).toBe(7);
  });

  describe('when nothing can be changed', () => {
    it.each([
      ['a read-only library', { readOnly: true }],
      ['something being done', { working: true }],
    ])('disables every control for %s', (_name, over) => {
      setup(over);
      for (const button of screen.getAllByRole('button')) {
        if (button.getAttribute('aria-pressed') !== null) continue; // choosing a curve to look at
        expect(button).toBeDisabled();
      }
      for (const s of screen.getAllByRole('slider')) {
        if (s.getAttribute('data-disabled') === null) {
          expect(s).toHaveAttribute('aria-disabled', 'true');
        }
      }
    });
  });

  it('has no accessibility violations', async () => {
    const { container } = setup();
    await expectNoA11yViolations(container);
  });
});
