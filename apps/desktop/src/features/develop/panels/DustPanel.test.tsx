import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { expectNoA11yViolations } from '../../../test-support';
import { makeRecipe } from '../../library/fake-ipc';
import { dustOf } from '../recipe-parts';
import { DustPanel } from './DustPanel';

type Props = Parameters<typeof DustPanel>[0];

const dusty = makeRecipe('x', { dust: { enabled: true, threshold: 0.3, grow_px: 4 } });

function setup(over: Partial<Props> = {}) {
  const mocks = { onChange: vi.fn() };
  const props: Props = {
    recipe: dusty,
    readOnly: false,
    working: false,
    flagOn: true,
    ...mocks,
    ...over,
  };
  return { props, mocks, ...render(<DustPanel {...props} />) };
}

const slider = (name: string) => screen.getByRole('slider', { name });
const toggle = () => screen.getByRole('checkbox', { name: 'Remove dust' });
const lastDust = (mocks: { onChange: ReturnType<typeof vi.fn> }) =>
  dustOf(mocks.onChange.mock.calls[mocks.onChange.mock.calls.length - 1]?.[0]);

describe('DustPanel', () => {
  it('shows the sensitivity as the threshold turned around, and the growth in pixels', () => {
    setup();
    expect(toggle()).toBeChecked();
    expect(slider('Sensitivity')).toHaveAttribute('aria-valuenow', '70');
    expect(slider('Sensitivity')).toHaveAttribute('aria-valuetext', '70 %');
    expect(slider('Widen spots by')).toHaveAttribute('aria-valuenow', '4');
    expect(slider('Widen spots by')).toHaveAttribute('aria-valuemax', '10');
    expect(slider('Widen spots by')).toHaveAttribute('aria-valuetext', '4 px');
  });

  it('shows the defaults for a recipe with no dust block', () => {
    setup({ recipe: { ...makeRecipe('x'), dust: undefined } as never });
    expect(toggle()).toBeChecked();
    expect(slider('Sensitivity')).toHaveAttribute('aria-valuenow', '50');
    expect(slider('Widen spots by')).toHaveAttribute('aria-valuenow', '2');
  });

  it('turns the sensitivity into a threshold without rounding noise', async () => {
    const user = userEvent.setup();
    const { mocks } = setup();
    slider('Sensitivity').focus();
    await user.keyboard('{ArrowRight}');
    expect(lastDust(mocks)).toEqual({ enabled: true, threshold: 0.29, grow_px: 4 });
    expect(mocks.onChange.mock.calls[0]?.[1]).toEqual({ dragging: false });
    await user.keyboard('{Home}');
    expect(lastDust(mocks).threshold).toBe(1);
    await user.keyboard('{End}');
    expect(lastDust(mocks).threshold).toBe(0);
  });

  it('moves the growth by whole pixels', async () => {
    const user = userEvent.setup();
    const { mocks } = setup();
    slider('Widen spots by').focus();
    await user.keyboard('{ArrowRight}');
    expect(lastDust(mocks).grow_px).toBe(5);
    await user.keyboard('{End}');
    expect(lastDust(mocks).grow_px).toBe(10);
    await user.keyboard('{Home}');
    expect(lastDust(mocks).grow_px).toBe(0);
  });

  it('switches off, which keeps the other values and turns the sliders off', async () => {
    const user = userEvent.setup();
    const { mocks, props, rerender } = setup();
    await user.click(toggle());
    expect(lastDust(mocks)).toEqual({ enabled: false, threshold: 0.3, grow_px: 4 });
    rerender(<DustPanel {...props} recipe={mocks.onChange.mock.calls[0]?.[0]} />);
    expect(toggle()).not.toBeChecked();
    for (const s of screen.getAllByRole('slider')) expect(s).toHaveAttribute('data-disabled');
  });

  it('keeps the rest of the recipe when it changes', async () => {
    const user = userEvent.setup();
    const { mocks } = setup();
    slider('Widen spots by').focus();
    await user.keyboard('{ArrowRight}');
    const next = mocks.onChange.mock.calls[0]?.[0] as Record<string, unknown>;
    const others = Object.keys(dusty).filter((key) => key !== 'dust');
    expect(others.length).toBeGreaterThan(2);
    for (const key of others) expect(next[key]).toEqual((dusty as Record<string, unknown>)[key]);
  });

  it('says so when dust removal is switched off in Settings, and only then', () => {
    const off = setup({ flagOn: false });
    expect(screen.getByRole('status')).toHaveTextContent('switched off in Settings');
    off.unmount();
    for (const flagOn of [true, null]) {
      const view = setup({ flagOn });
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
      view.unmount();
    }
  });

  it('explains that the preview shows a smaller copy of the plane', () => {
    setup();
    const note = screen.getByText(/smaller copy of the plane/);
    expect(screen.getByRole('group', { name: 'Dust removal' })).toHaveAccessibleDescription(
      note.textContent ?? '',
    );
  });

  it('resets to the defaults, and has nothing to reset at them', async () => {
    const user = userEvent.setup();
    const { props, mocks, rerender } = setup();
    await user.click(screen.getByRole('button', { name: 'Reset dust removal' }));
    expect(lastDust(mocks)).toEqual({ enabled: true, threshold: 0.5, grow_px: 2 });
    rerender(<DustPanel {...props} recipe={makeRecipe('x')} />);
    expect(screen.getByRole('button', { name: 'Reset dust removal' })).toBeDisabled();
  });

  describe('when nothing can be changed', () => {
    it.each([
      ['a read-only library', { readOnly: true }],
      ['something being done', { working: true }],
    ])('disables every control for %s', (_name, over) => {
      setup(over);
      expect(toggle()).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Reset dust removal' })).toBeDisabled();
      for (const s of screen.getAllByRole('slider')) expect(s).toHaveAttribute('data-disabled');
    });
  });

  it('has no accessibility violations', async () => {
    const { container } = setup({ flagOn: false });
    await expectNoA11yViolations(container);
  });
});
