import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../test-support';
import { makeRecipe } from '../../library/fake-ipc';
import { curvesOf, toneOf } from '../recipe-parts';
import { TonePanel } from './TonePanel';

type Props = Parameters<typeof TonePanel>[0];

function setup(over: Partial<Props> = {}) {
  const mocks = { onChange: vi.fn() };
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

  it('resets every slider, but not the curves', async () => {
    const user = userEvent.setup();
    const { mocks } = setup({
      recipe: makeRecipe('x', {
        tone: {
          exposure_ev: 1,
          whites: 30,
          curve: {
            master: [
              [0, 0.1],
              [1, 1],
            ],
            r: [],
            g: [],
            b: [],
          },
        },
      }),
    });
    await user.click(screen.getByRole('button', { name: 'Reset tone' }));
    const [recipe, options] = mocks.onChange.mock.calls[0] ?? [];
    expect(Object.values(toneOf(recipe)).every((v) => v === 0)).toBe(true);
    expect(curvesOf(recipe).master[0]).toEqual([0, 0.1]);
    expect(options).toEqual({ dragging: false });
  });

  it('has nothing to reset when every slider is at 0', () => {
    setup({ recipe: makeRecipe('x') });
    expect(screen.getByRole('button', { name: 'Reset tone' })).toBeDisabled();
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
