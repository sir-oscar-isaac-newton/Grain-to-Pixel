// Helpers shared by component tests.
import { configureAxe } from 'vitest-axe';
import { expect } from 'vitest';

/** axe-core with the rules jsdom can run (it cannot compute colours, so contrast is skipped). */
const axe = configureAxe({ rules: { 'color-contrast': { enabled: false } } });

/** Fails with the list of accessibility violations found in `container`, if any. */
export async function expectNoA11yViolations(container: Element) {
  const results = await axe(container);
  expect(
    results.violations.map(
      (v) => `${v.id}: ${v.help} (${v.nodes.map((n) => n.target).join(', ')})`,
    ),
  ).toEqual([]);
}

/**
 * Gives elements a size, since jsdom does no layout. Returns a function that undoes it.
 * Used for the virtualized grid, which needs a width (columns) and a height (visible rows).
 */
export function mockLayout(width: number, height: number): () => void {
  const originals = new Map<string, PropertyDescriptor | undefined>();
  const define = (proto: object, name: string, value: number) => {
    originals.set(name, Object.getOwnPropertyDescriptor(proto, name));
    Object.defineProperty(proto, name, { configurable: true, get: () => value });
  };
  define(HTMLElement.prototype, 'offsetWidth', width);
  define(HTMLElement.prototype, 'offsetHeight', height);
  define(Element.prototype, 'clientWidth', width);
  define(Element.prototype, 'clientHeight', height);
  return () => {
    for (const [name, descriptor] of originals) {
      const proto = name.startsWith('offset') ? HTMLElement.prototype : Element.prototype;
      if (descriptor) Object.defineProperty(proto, name, descriptor);
      else Reflect.deleteProperty(proto, name);
    }
  };
}
