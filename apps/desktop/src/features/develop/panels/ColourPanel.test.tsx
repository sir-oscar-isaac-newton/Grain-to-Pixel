import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../test-support';
import { makeRecipe } from '../../library/fake-ipc';
import { bandCss, hueGradientCss, tintCss } from '../hue-colour';
import { colourOf, mixerOf, monoOf, splitToneOf } from '../recipe-parts';
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

  describe('split toning', () => {
    const toned = makeRecipe('x', {
      color: {
        split_tone: {
          highlight_hue: 30,
          highlight_strength: 60,
          shadow_hue: 210,
          shadow_strength: 40,
          balance: -25,
        },
      },
    });

    it('has its own section with five sliders showing the recipe’s values', () => {
      setup({ recipe: toned });
      expect(screen.getByRole('heading', { name: 'Split toning' })).toBeInTheDocument();
      expect(slider('Highlights hue')).toHaveAttribute('aria-valuenow', '30');
      expect(slider('Highlights hue')).toHaveAttribute('aria-valuemax', '360');
      expect(slider('Highlights hue')).toHaveAttribute('aria-valuetext', '30°');
      expect(slider('Highlights strength')).toHaveAttribute('aria-valuenow', '60');
      expect(slider('Highlights strength')).toHaveAttribute('aria-valuemax', '100');
      expect(slider('Shadows hue')).toHaveAttribute('aria-valuenow', '210');
      expect(slider('Shadows strength')).toHaveAttribute('aria-valuenow', '40');
      expect(slider('Balance')).toHaveAttribute('aria-valuenow', '-25');
      expect(slider('Balance')).toHaveAttribute('aria-valuemin', '-100');
    });

    it('shows the defaults for a recipe without it', () => {
      setup();
      expect(slider('Highlights hue')).toHaveAttribute('aria-valuenow', '40');
      expect(slider('Highlights strength')).toHaveAttribute('aria-valuenow', '0');
      expect(slider('Shadows hue')).toHaveAttribute('aria-valuenow', '220');
      expect(slider('Balance')).toHaveAttribute('aria-valuenow', '0');
    });

    it.each([
      ['Highlights hue', 'highlight_hue', 31],
      ['Highlights strength', 'highlight_strength', 61],
      ['Shadows hue', 'shadow_hue', 211],
      ['Shadows strength', 'shadow_strength', 41],
      ['Balance', 'balance', -24],
    ] as const)('changes the recipe with %s and keeps the rest', async (name, key, want) => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: toned });
      slider(name).focus();
      await user.keyboard('{ArrowRight}');
      const [recipe, options] = mocks.onChange.mock.calls[0] ?? [];
      expect(splitToneOf(recipe)).toEqual({ ...splitToneOf(toned), [key]: want });
      expect(options).toEqual({ dragging: false });
    });

    it('resets one slider to the automatic recipe’s value, merging the change', () => {
      const auto = makeRecipe('auto');
      const { mocks } = setup({ recipe: toned, auto });
      fireEvent.doubleClick(slider('Highlights strength'));
      const [recipe, options] = mocks.onChange.mock.calls.at(-1) ?? [];
      expect(splitToneOf(recipe)).toEqual({ ...splitToneOf(toned), highlight_strength: 0 });
      expect(options).toEqual({ dragging: false, merge: true });
    });

    it('is covered by the panel reset', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: toned, auto: makeRecipe('auto') });
      await user.click(screen.getByRole('button', { name: /Reset Colour/ }));
      const [recipe] = mocks.onChange.mock.calls.at(-1) ?? [];
      expect(splitToneOf(recipe)).toEqual(splitToneOf(makeRecipe('auto')));
    });

    it('stays enabled and usable in black and white', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({
        recipe: makeRecipe('x', { ...mono, color: toned.color }),
      });
      expect(screen.queryByRole('slider', { name: 'Saturation' })).not.toBeInTheDocument();
      for (const name of [
        'Highlights hue',
        'Highlights strength',
        'Shadows hue',
        'Shadows strength',
        'Balance',
      ]) {
        expect(slider(name)).not.toHaveAttribute('data-disabled');
      }
      slider('Shadows strength').focus();
      await user.keyboard('{ArrowRight}');
      const [recipe] = mocks.onChange.mock.calls[0] ?? [];
      expect(splitToneOf(recipe).shadow_strength).toBe(41);
      expect(monoOf(recipe).enabled).toBe(true);
    });
  });

  describe('colour mixer', () => {
    const BANDS = ['Red', 'Orange', 'Yellow', 'Green', 'Aqua', 'Blue', 'Purple', 'Magenta'];
    const mixed = makeRecipe('x', {
      color: {
        mixer: {
          hue: [10, 0, 0, 0, 0, 0, 0, -20],
          saturation: [0, -30, 0, 0, 0, 0, 40, 0],
          lightness: [0, 0, 5, 0, 0, 0, 0, 0],
        },
      },
    });
    const mode = (name: string) => screen.getByRole('button', { name });

    it('has its own section before split toning, with a three-way switch on hue', () => {
      setup({ recipe: mixed });
      const headings = screen.getAllByRole('heading', { level: 3 }).map((h) => h.textContent);
      expect(headings.indexOf('Colour mixer')).toBeGreaterThan(-1);
      expect(headings.indexOf('Colour mixer')).toBe(headings.indexOf('Split toning') - 1);
      const group = screen.getByRole('group', { name: 'Change' });
      expect(group).toContainElement(mode('Hue'));
      expect(group).toContainElement(mode('Saturation'));
      expect(group).toContainElement(mode('Lightness'));
      expect(mode('Hue')).toHaveAttribute('aria-pressed', 'true');
      expect(mode('Saturation')).toHaveAttribute('aria-pressed', 'false');
      expect(mode('Lightness')).toHaveAttribute('aria-pressed', 'false');
    });

    it('shows eight sliders, one per band, for the chosen mode', async () => {
      const user = userEvent.setup();
      setup({ recipe: mixed });
      for (const band of BANDS) {
        expect(slider(band)).toHaveAttribute('aria-valuemin', '-100');
        expect(slider(band)).toHaveAttribute('aria-valuemax', '100');
      }
      expect(slider('Red')).toHaveAttribute('aria-valuenow', '10');
      expect(slider('Magenta')).toHaveAttribute('aria-valuenow', '-20');
      expect(slider('Orange')).toHaveAttribute('aria-valuenow', '0');

      await user.click(mode('Saturation'));
      expect(mode('Saturation')).toHaveAttribute('aria-pressed', 'true');
      expect(mode('Hue')).toHaveAttribute('aria-pressed', 'false');
      expect(slider('Red')).toHaveAttribute('aria-valuenow', '0');
      expect(slider('Orange')).toHaveAttribute('aria-valuenow', '-30');
      expect(slider('Purple')).toHaveAttribute('aria-valuenow', '40');

      await user.click(mode('Lightness'));
      expect(slider('Yellow')).toHaveAttribute('aria-valuenow', '5');
      expect(slider('Orange')).toHaveAttribute('aria-valuenow', '0');
    });

    it('shows zeros for a recipe without a mixer', () => {
      setup();
      for (const band of BANDS) expect(slider(band)).toHaveAttribute('aria-valuenow', '0');
    });

    it.each([
      ['Hue', 'hue', 'Green', 3, 1],
      ['Saturation', 'saturation', 'Aqua', 4, 1],
      ['Lightness', 'lightness', 'Magenta', 7, 1],
    ] as const)('changes only that band of the %s list', async (label, key, band, index, step) => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: mixed });
      await user.click(mode(label));
      slider(band).focus();
      await user.keyboard('{ArrowRight}');
      const [recipe, options] = mocks.onChange.mock.calls[0] ?? [];
      const before = mixerOf(mixed);
      const after = mixerOf(recipe);
      expect(after[key][index]).toBe((before[key][index] ?? 0) + step);
      expect(after[key].filter((_, i) => i !== index)).toEqual(
        before[key].filter((_, i) => i !== index),
      );
      for (const other of (['hue', 'saturation', 'lightness'] as const).filter((k) => k !== key)) {
        expect(after[other]).toEqual(before[other]);
      }
      expect(options).toEqual({ dragging: false });
      // The rest of the colour block is as it was.
      expect(colourOf(recipe)).toEqual(colourOf(mixed));
    });

    it('resets one slider to the automatic recipe’s value, merging the change', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: mixed, auto: makeRecipe('auto') });
      await user.click(mode('Saturation'));
      fireEvent.doubleClick(slider('Purple'));
      const [recipe, options] = mocks.onChange.mock.calls.at(-1) ?? [];
      expect(mixerOf(recipe)).toEqual({
        ...mixerOf(mixed),
        saturation: [0, -30, 0, 0, 0, 0, 0, 0],
      });
      expect(options).toEqual({ dragging: false, merge: true });
    });

    it('is covered by the panel reset', async () => {
      const user = userEvent.setup();
      const { mocks } = setup({ recipe: mixed, auto: makeRecipe('auto') });
      await user.click(screen.getByRole('button', { name: /Reset Colour/ }));
      const [recipe] = mocks.onChange.mock.calls.at(-1) ?? [];
      expect(mixerOf(recipe)).toEqual(mixerOf(makeRecipe('auto')));
      expect(mixerOf(recipe).hue).toEqual([0, 0, 0, 0, 0, 0, 0, 0]);
    });

    it('is hidden in black and white, which it does not act on', () => {
      setup({ recipe: makeRecipe('x', { ...mono, color: mixed.color }) });
      expect(screen.queryByRole('heading', { name: 'Colour mixer' })).not.toBeInTheDocument();
      expect(screen.queryByRole('group', { name: 'Change' })).not.toBeInTheDocument();
      expect(screen.queryByRole('slider', { name: 'Orange' })).not.toBeInTheDocument();
    });

    it('disables the sliders when nothing can be changed', () => {
      setup({ recipe: mixed, readOnly: true });
      for (const band of BANDS) expect(slider(band)).toHaveAttribute('data-disabled');
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

  describe('swatches', () => {
    const hueSliders = ['Highlights hue', 'Shadows hue'];
    const BANDS = ['Red', 'Orange', 'Yellow', 'Green', 'Aqua', 'Blue', 'Purple', 'Magenta'];
    /** The nearest element around the slider that holds one marked with `selector`. */
    const decorationOf = (name: string, selector: string) => {
      let node: HTMLElement | null = slider(name);
      while (node && !node.querySelector(selector)) node = node.parentElement;
      return node?.querySelector(selector) ?? null;
    };
    const withHue = (hue: number) =>
      makeRecipe('x', { color: { split_tone: { highlight_hue: hue, shadow_hue: hue + 100 } } });

    it('gives the two hue sliders a hue gradient and a swatch that follows the hue', () => {
      const { props, rerender } = setup({ recipe: withHue(0) });
      for (const name of hueSliders) {
        expect(slider(name)).toHaveAttribute('data-gradient');
        expect(
          decorationOf(name, '[data-gradient]:not([role="slider"])')?.getAttribute('style'),
        ).toContain(hueGradientCss());
        expect(decorationOf(name, '[data-swatch]')).not.toBeNull();
      }
      const swatch = (name: string) => decorationOf(name, '[data-swatch]')?.getAttribute('style');
      expect(swatch('Highlights hue')).toContain(tintCss(0));
      expect(swatch('Shadows hue')).toContain(tintCss(100));
      rerender(<ColourPanel {...props} recipe={withHue(240)} />);
      expect(swatch('Highlights hue')).toContain(tintCss(240));
      expect(swatch('Highlights hue')).not.toContain(tintCss(0));
    });

    it.each(['Hue', 'Saturation', 'Lightness'])(
      'gives each mixer slider its band swatch, with no gradient, in %s mode',
      async (modeName) => {
        const user = userEvent.setup();
        setup();
        await user.click(screen.getByRole('button', { name: modeName }));
        BANDS.forEach((band, index) => {
          expect(decorationOf(band, '[data-swatch]')?.getAttribute('style')).toContain(
            bandCss(index),
          );
          expect(slider(band)).not.toHaveAttribute('data-gradient');
        });
      },
    );

    it('leaves the other sliders without a swatch or a gradient', () => {
      const { container } = setup();
      expect(container.querySelectorAll('[data-swatch]')).toHaveLength(
        hueSliders.length + BANDS.length,
      );
      // The track, the range and the thumb of each hue slider.
      expect(container.querySelectorAll('[data-gradient]')).toHaveLength(hueSliders.length * 3);
      for (const name of ['Temperature', 'Saturation', 'Highlights strength', 'Balance']) {
        expect(slider(name)).not.toHaveAttribute('data-gradient');
      }
    });
  });

  it('has no accessibility violations, also in black and white and while picking', async () => {
    const { container, props, rerender } = setup();
    await expectNoA11yViolations(container);
    rerender(<ColourPanel {...props} recipe={mono} picking pickReady />);
    await expectNoA11yViolations(container);
  });
});
