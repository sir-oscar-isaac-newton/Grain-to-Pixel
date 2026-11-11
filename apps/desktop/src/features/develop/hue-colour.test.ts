import { describe, expect, it } from 'vitest';
import mixerSource from '../../../../../crates/gtp-edit/src/mixer.rs?raw';
import { MIXER_BAND_HUES, bandCss, hueGradientCss, tintCss } from './hue-colour';

const channels = (css: string): [number, number, number] => {
  const match = /^rgb\((\d+) (\d+) (\d+)\)$/.exec(css);
  if (!match) throw new Error(`not an rgb() colour: ${css}`);
  return [Number(match[1]), Number(match[2]), Number(match[3])];
};

describe('tintCss', () => {
  it('is red at 0, green at 120 and blue at 240', () => {
    const [r0, g0, b0] = channels(tintCss(0));
    expect(r0).toBe(255);
    expect(Math.max(g0, b0)).toBeLessThan(40);
    const [r1, g1, b1] = channels(tintCss(120));
    expect(g1).toBe(255);
    expect(Math.max(r1, b1)).toBeLessThan(40);
    const [r2, g2, b2] = channels(tintCss(240));
    expect(b2).toBe(255);
    expect(Math.max(r2, g2)).toBeLessThan(40);
  });

  it('is yellow at 60: red and green high, blue low', () => {
    const [r, g, b] = channels(tintCss(60));
    expect(r).toBeGreaterThan(200);
    expect(g).toBeGreaterThan(200);
    expect(b).toBeLessThan(40);
  });

  it('wraps at a full turn, for negative and for large hues', () => {
    expect(tintCss(360)).toBe(tintCss(0));
    expect(tintCss(-120)).toBe(tintCss(240));
    expect(tintCss(480)).toBe(tintCss(120));
  });

  it('has whole channels from 0 to 255 at every degree, the largest at full brightness', () => {
    for (let hue = 0; hue <= 360; hue += 1) {
      const values = channels(tintCss(hue));
      for (const value of values) {
        expect(Number.isInteger(value)).toBe(true);
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(255);
      }
      expect(Math.max(...values)).toBe(255);
    }
  });
});

describe('hueGradientCss', () => {
  it('runs left to right with a stop every 30 degrees, 13 in all, from red to red', () => {
    const css = hueGradientCss();
    expect(css.startsWith('linear-gradient(to right, ')).toBe(true);
    const stops = css.match(/rgb\(\d+ \d+ \d+\)/g) ?? [];
    expect(stops).toHaveLength(13);
    expect(stops).toEqual(Array.from({ length: 13 }, (_, i) => tintCss(i * 30)));
    expect(stops[0]).toBe(stops[12]);
  });
});

describe('bandCss', () => {
  it.each(MIXER_BAND_HUES.map((hue, index) => [index, hue] as const))(
    'is an oklch colour at the hue of band %i',
    (index, hue) => {
      expect(bandCss(index)).toBe(`oklch(0.72 0.15 ${hue}deg)`);
    },
  );

  it('has the band hues of the Rust mixer', () => {
    const match = /pub const MIXER_BAND_HUES: \[f32; MIXER_BANDS\] =\s*\[([^\]]*)\]/.exec(
      mixerSource,
    );
    expect(match).not.toBeNull();
    const rust = (match?.[1] ?? '').split(',').map((n) => Number(n.trim()));
    expect(rust).toEqual([...MIXER_BAND_HUES]);
  });
});
