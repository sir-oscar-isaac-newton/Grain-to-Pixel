import { describe, expect, it } from 'vitest';
import colorSource from '../../../../../crates/gtp-edit/src/color.rs?raw';
import mixerSource from '../../../../../crates/gtp-edit/src/mixer.rs?raw';
import {
  AXIS_STOPS,
  MID_GREY,
  MIXER_BAND_HUES,
  MIXER_HUE_MAX_DEG,
  axisGradientCss,
  bandCss,
  bandHueGradientCss,
  hueGradientCss,
  tintCss,
} from './hue-colour';

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

describe('bandHueGradientCss', () => {
  const stop = (hue: number) => `oklch(0.72 0.15 ${hue}deg)`;

  it.each(MIXER_BAND_HUES.map((hue, index) => [index, hue] as const))(
    'has the three stops of band %i: 30 degrees down, the band, 30 degrees up',
    (index, hue) => {
      const low = (hue - 30 + 360) % 360;
      const high = (hue + 30) % 360;
      expect(bandHueGradientCss(index)).toBe(
        `linear-gradient(to right, ${stop(low)}, ${stop(hue)}, ${stop(high)})`,
      );
    },
  );

  it('wraps below 0 and from 360 on', () => {
    expect(bandHueGradientCss(0)).toBe(
      `linear-gradient(to right, ${stop(359)}, ${stop(29)}, ${stop(59)})`,
    );
    expect(bandHueGradientCss(7)).toBe(
      `linear-gradient(to right, ${stop(298)}, ${stop(328)}, ${stop(358)})`,
    );
  });

  it('has the largest hue turn of the Rust mixer', () => {
    const match = /pub const MIXER_HUE_MAX_DEG: f32 = ([\d.]+);/.exec(mixerSource);
    expect(match).not.toBeNull();
    expect(Number(match?.[1])).toBe(MIXER_HUE_MAX_DEG);
  });
});

describe('axisGradientCss', () => {
  const stops = (axis: 'red_cyan' | 'green_magenta' | 'blue_yellow') =>
    (axisGradientCss(axis).match(/rgb\(\d+ \d+ \d+\)/g) ?? []).map(channels);

  it('has the exact three stops of red_cyan', () => {
    expect(axisGradientCss('red_cyan')).toBe(
      'linear-gradient(to right, rgb(0 143 139), rgb(118 118 118), rgb(190 88 98))',
    );
  });

  it('has the exact three stops of green_magenta', () => {
    expect(axisGradientCss('green_magenta')).toBe(
      'linear-gradient(to right, rgb(160 74 142), rgb(118 118 118), rgb(0 168 91))',
    );
  });

  it('has the exact three stops of blue_yellow', () => {
    expect(axisGradientCss('blue_yellow')).toBe(
      'linear-gradient(to right, rgb(141 138 75), rgb(118 118 118), rgb(93 99 167))',
    );
  });

  it('has the same plain mid grey in the middle of all three', () => {
    const middles = (['red_cyan', 'green_magenta', 'blue_yellow'] as const).map((a) => stops(a)[1]);
    expect(middles[0]).toEqual(middles[1]);
    expect(middles[1]).toEqual(middles[2]);
    const [r, g, b] = middles[0] ?? [0, 0, 0];
    expect(r).toBe(g);
    expect(g).toBe(b);
    expect(MID_GREY).toBe(0.18);
  });

  it('adds cyan at -100 and red at +100 on red_cyan', () => {
    const [low, , high] = stops('red_cyan');
    expect(low?.[1]).toBeGreaterThan(low?.[0] ?? 0);
    expect(low?.[2]).toBeGreaterThan(low?.[0] ?? 0);
    expect(high?.[0]).toBeGreaterThan(high?.[1] ?? 0);
    expect(high?.[0]).toBeGreaterThan(high?.[2] ?? 0);
  });

  it('adds magenta at -100 and green at +100 on green_magenta', () => {
    const [low, , high] = stops('green_magenta');
    expect(low?.[0]).toBeGreaterThan(low?.[1] ?? 0);
    expect(low?.[2]).toBeGreaterThan(low?.[1] ?? 0);
    expect(high?.[1]).toBeGreaterThan(high?.[0] ?? 0);
    expect(high?.[1]).toBeGreaterThan(high?.[2] ?? 0);
  });

  it('adds yellow at -100 and blue at +100 on blue_yellow', () => {
    const [low, , high] = stops('blue_yellow');
    expect(low?.[0]).toBeGreaterThan(low?.[2] ?? 0);
    expect(low?.[1]).toBeGreaterThan(low?.[2] ?? 0);
    expect(high?.[2]).toBeGreaterThan(high?.[0] ?? 0);
    expect(high?.[2]).toBeGreaterThan(high?.[1] ?? 0);
  });

  it('has the axis stops of the Rust pipeline', () => {
    const match = /pub const AXIS_STOPS: f32 = ([\d.]+);/.exec(colorSource);
    expect(match).not.toBeNull();
    expect(Number(match?.[1])).toBe(AXIS_STOPS);
  });
});
