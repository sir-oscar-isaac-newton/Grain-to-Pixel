import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../test-support';
import { makeRecipe, mergeFields } from '../../library/fake-ipc';
import {
  PAPER_GRADES,
  SPLIT_TONE_DEFAULT,
  TONING_PRESETS,
  isBlackAndWhite,
  splitToneOf,
  toneOf,
  withSplitTone,
  withTone,
} from '../recipe-parts';
import { BlackWhitePanel } from './BlackWhitePanel';

type Props = Parameters<typeof BlackWhitePanel>[0];

const bw = (over: Record<string, unknown> = {}) =>
  makeRecipe('x', {
    invert: { black: [0, 0, 0], white: [1, 1, 1], combine: [0.3, 0.5, 0.2] },
    ...over,
  });

function setup(over: Partial<Props> = {}) {
  const mocks = { onChange: vi.fn() };
  const props: Props = { recipe: bw(), readOnly: false, working: false, ...mocks, ...over };
  return { props, mocks, ...render(<BlackWhitePanel {...props} />) };
}

const slider = (name: string) => screen.getByRole('slider', { name });
const button = (name: string) => screen.getByRole('button', { name });
const lastRecipe = (mocks: { onChange: ReturnType<typeof vi.fn> }) =>
  mocks.onChange.mock.calls.at(-1)?.[0];

describe('BlackWhitePanel', () => {
  it('is called Black and white and has no colour controls', () => {
    setup();
    expect(screen.getByRole('heading', { name: 'Black and white' })).toBeInTheDocument();
    for (const name of ['Temperature', 'Tint', 'Saturation', 'Vibrance', 'Cyan / red']) {
      expect(screen.queryByRole('slider', { name })).not.toBeInTheDocument();
    }
    expect(screen.queryByRole('button', { name: 'Pick neutral grey' })).not.toBeInTheDocument();
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Colour mixer' })).not.toBeInTheDocument();
  });

  describe('paper grade', () => {
    it.each(Object.entries(PAPER_GRADES))('grade %s sets the contrast to %s', async (n, want) => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: bw({ tone: { exposure_ev: 0.5, contrast: 10 } }) });
      await user.click(button(`Grade ${n}`));
      const recipe = lastRecipe(mocks);
      expect(toneOf(recipe).contrast).toBe(want);
      expect(toneOf(recipe).exposure_ev).toBe(0.5);
      expect(mocks.onChange.mock.calls.at(-1)?.[1]).toEqual({ dragging: false });
    });

    it('shows the grade whose contrast the recipe has exactly, and none otherwise', () => {
      const { rerender, props } = setup({ recipe: withTone(bw(), { contrast: 50 }) });
      const pressed = () =>
        screen
          .getAllByRole('button', { name: /^Grade / })
          .filter((b) => b.getAttribute('aria-pressed') === 'true')
          .map((b) => b.textContent);
      expect(pressed()).toEqual(['Grade 4']);
      rerender(<BlackWhitePanel {...props} recipe={withTone(bw(), { contrast: 30 })} />);
      expect(pressed()).toEqual([]);
    });

    it('says the contrast of the Tone panel stays editable', () => {
      setup();
      expect(screen.getByRole('group', { name: 'Paper grade' })).toHaveAccessibleDescription(
        /contrast in the Tone panel stays editable/,
      );
    });
  });

  describe('toning', () => {
    it.each(Object.entries(TONING_PRESETS))(
      '%s sets all five split toning values',
      async (name, want) => {
        const user = userEvent.setup();
        const label = (name[0] ?? '').toUpperCase() + name.slice(1);
        const { mocks } = setup({
          recipe: withSplitTone(bw(), { highlight_strength: 77, balance: 5 }),
        });
        await user.click(button(label));
        expect(splitToneOf(lastRecipe(mocks))).toEqual(want);
        expect(mocks.onChange.mock.calls.at(-1)?.[1]).toEqual({ dragging: false });
      },
    );

    it('None puts the strengths back to 0', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: withSplitTone(bw(), TONING_PRESETS.sepia) });
      await user.click(button('None'));
      expect(splitToneOf(lastRecipe(mocks))).toEqual(SPLIT_TONE_DEFAULT);
    });

    it('shows the preset that matches exactly, and none once a slider has moved', () => {
      const pressed = (name: string) => button(name).getAttribute('aria-pressed');
      const { rerender, props } = setup({ recipe: withSplitTone(bw(), TONING_PRESETS.sepia) });
      expect(pressed('Sepia')).toBe('true');
      expect(pressed('None')).toBe('false');
      rerender(
        <BlackWhitePanel
          {...props}
          recipe={withSplitTone(bw(), { ...TONING_PRESETS.sepia, balance: 21 })}
        />,
      );
      expect(pressed('Sepia')).toBe('false');
    });

    it('keeps the sliders editable after a preset', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: withSplitTone(bw(), TONING_PRESETS.selenium) });
      expect(slider('Shadows strength')).toHaveAttribute('aria-valuenow', '25');
      slider('Shadows strength').focus();
      await user.keyboard('{ArrowRight}');
      expect(splitToneOf(lastRecipe(mocks))).toEqual({
        ...TONING_PRESETS.selenium,
        shadow_strength: 26,
      });
    });

    it('resets one slider to the automatic recipe value, merging the change', () => {
      const { mocks } = setup({
        recipe: withSplitTone(bw(), TONING_PRESETS.sepia),
        auto: bw(),
      });
      fireEvent.doubleClick(slider('Highlights strength'));
      const [recipe, options] = mocks.onChange.mock.calls.at(-1) ?? [];
      expect(splitToneOf(recipe)).toEqual({ ...TONING_PRESETS.sepia, highlight_strength: 0 });
      expect(options).toEqual({ dragging: false, merge: true });
    });
  });

  it('resets the split toning to the automatic recipe and leaves the grade', async () => {
    const user = userEvent.setup();
    const recipe = withTone(withSplitTone(bw(), TONING_PRESETS.cyanotype), { contrast: 50 });
    const { mocks } = setup({ recipe, auto: bw() });
    await user.click(button('Reset Black and white to automatic'));
    const next = lastRecipe(mocks);
    expect(splitToneOf(next)).toEqual(splitToneOf(bw()));
    expect(toneOf(next).contrast).toBe(50);
  });

  it('has the reset off without an automatic recipe', () => {
    setup();
    expect(button('Reset Black and white to automatic')).toBeDisabled();
  });

  it.each([
    ['read-only', { readOnly: true }],
    ['working', { working: true }],
  ])('disables everything when %s', (_name, over) => {
    setup({ ...over, auto: bw() });
    for (const b of screen.getAllByRole('button')) expect(b).toBeDisabled();
    for (const s of screen.getAllByRole('slider')) expect(s).toHaveAttribute('data-disabled');
  });

  it('has no accessibility violations', async () => {
    const { container } = setup({ recipe: withSplitTone(bw(), TONING_PRESETS.sepia), auto: bw() });
    await expectNoA11yViolations(container);
  });

  it('knows a black-and-white frame by its combined inversion', () => {
    expect(isBlackAndWhite(bw())).toBe(true);
    expect(isBlackAndWhite(makeRecipe('x'))).toBe(false);
  });

  describe('apply settings and presets', () => {
    it('carry the grade in the Tone group and the toning in the Split toning group', () => {
      const source = withTone(withSplitTone(bw(), TONING_PRESETS.sepia), { contrast: 50 });
      const target = makeRecipe('target');
      const tone = mergeFields(target, source, ['Tone']);
      expect(toneOf(tone).contrast).toBe(PAPER_GRADES[4]);
      expect(splitToneOf(tone)).toEqual(SPLIT_TONE_DEFAULT);
      const toned = mergeFields(target, source, ['SplitTone']);
      expect(splitToneOf(toned)).toEqual(TONING_PRESETS.sepia);
      expect(toneOf(toned).contrast).toBe(0);
      expect(isBlackAndWhite(toned)).toBe(false);
    });
  });
});
