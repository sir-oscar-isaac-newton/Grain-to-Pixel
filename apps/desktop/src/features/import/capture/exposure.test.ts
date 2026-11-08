import { describe, expect, it } from 'vitest';
import type { ExposureInfo } from '../../../shared/ipc';
import { exposureDiffers, formatExposure, formatShutterChoice } from './exposure';

const base: ExposureInfo = {
  exposure_time: { num: 1, den: 60 },
  iso: 200,
  f_number: { num: 56, den: 10 },
};

describe('exposureDiffers', () => {
  it('is false for the same settings, also as other fractions', () => {
    expect(exposureDiffers(base, { ...base, exposure_time: { num: 2, den: 120 } })).toBe(false);
  });

  it('is true when the shutter, the ISO or the aperture differ', () => {
    expect(exposureDiffers(base, { ...base, exposure_time: { num: 1, den: 125 } })).toBe(true);
    expect(exposureDiffers(base, { ...base, iso: 400 })).toBe(true);
    expect(exposureDiffers(base, { ...base, f_number: { num: 8, den: 1 } })).toBe(true);
  });

  it('is false when a setting or a whole side is unknown', () => {
    expect(exposureDiffers(base, { exposure_time: null, iso: null, f_number: null })).toBe(false);
    expect(exposureDiffers(null, base)).toBe(false);
    expect(exposureDiffers(base, null)).toBe(false);
  });
});

describe('formatExposure', () => {
  it('writes the shutter time, ISO and aperture as a photographer does', () => {
    expect(formatExposure(base)).toBe('1/60 s, ISO 200, f/5.6');
    expect(
      formatExposure({
        exposure_time: { num: 1, den: 100 },
        iso: 100,
        f_number: { num: 8, den: 1 },
      }),
    ).toBe('1/100 s, ISO 100, f/8');
  });

  it('writes long and odd shutter times without a fraction that is not one', () => {
    const at = (num: number, den: number) =>
      formatExposure({ exposure_time: { num, den }, iso: null, f_number: null });
    expect(at(2, 1)).toBe('2 s');
    expect(at(3, 2)).toBe('1.5 s');
    expect(at(1, 1)).toBe('1 s');
    expect(at(2, 100)).toBe('1/50 s');
    expect(at(3, 10)).toBe('0.3 s');
    expect(at(10, 1)).toBe('10 s');
  });

  it('writes the aperture to one decimal without a trailing zero', () => {
    const at = (num: number, den: number) =>
      formatExposure({ exposure_time: null, iso: null, f_number: { num, den } });
    expect(at(80, 10)).toBe('f/8');
    expect(at(56, 10)).toBe('f/5.6');
    expect(at(14, 10)).toBe('f/1.4');
    expect(at(11, 1)).toBe('f/11');
  });

  it('leaves out what is unknown, and is null when nothing is known', () => {
    expect(formatExposure({ exposure_time: null, iso: 400, f_number: null })).toBe('ISO 400');
    expect(formatExposure({ exposure_time: null, iso: null, f_number: null })).toBeNull();
    expect(
      formatExposure({ exposure_time: { num: 1, den: 0 }, iso: null, f_number: null }),
    ).toBeNull();
    expect(formatExposure(null)).toBeNull();
  });
});

describe('formatShutterChoice', () => {
  const table: [string, string][] = [
    ['0.0002s', '1/4000 s'],
    ['0.0003s', '1/3200 s'],
    ['0.0004s', '1/2500 s'],
    ['0.0005s', '1/2000 s'],
    ['0.0006s', '1/1600 s'],
    ['0.0008s', '1/1250 s'],
    ['0.0010s', '1/1000 s'],
    ['0.0012s', '1/800 s'],
    ['0.0015s', '1/640 s'],
    ['0.0020s', '1/500 s'],
    ['0.0025s', '1/400 s'],
    ['0.0031s', '1/320 s'],
    ['0.0040s', '1/250 s'],
    ['0.0050s', '1/200 s'],
    ['0.0062s', '1/160 s'],
    ['0.0080s', '1/125 s'],
    ['0.0100s', '1/100 s'],
    ['0.0125s', '1/80 s'],
    ['0.0166s', '1/60 s'],
    ['0.0200s', '1/50 s'],
    ['0.0250s', '1/40 s'],
    ['0.0333s', '1/30 s'],
    ['0.0400s', '1/25 s'],
    ['0.0500s', '1/20 s'],
    ['0.0666s', '1/15 s'],
    ['0.0769s', '1/13 s'],
    ['0.1000s', '1/10 s'],
    ['0.1250s', '1/8 s'],
    ['0.1666s', '1/6 s'],
    ['0.2000s', '1/5 s'],
    ['0.2500s', '1/4 s'],
    ['0.3333s', '1/3 s'],
    ['0.4000s', '0.4 s'],
    ['0.5000s', '0.5 s'],
    ['0.6250s', '0.6 s'],
    ['0.7692s', '0.8 s'],
    ['1.0000s', '1 s'],
    ['1.3000s', '1.3 s'],
    ['1.6000s', '1.6 s'],
    ['2.0000s', '2 s'],
    ['2.5000s', '2.5 s'],
    ['3.0000s', '3 s'],
    ['4.0000s', '4 s'],
    ['5.0000s', '5 s'],
    ['6.0000s', '6 s'],
    ['8.0000s', '8 s'],
    ['10.0000s', '10 s'],
    ['13.0000s', '13 s'],
    ['15.0000s', '15 s'],
    ['20.0000s', '20 s'],
    ['25.0000s', '25 s'],
    ['30.0000s', '30 s'],
    ['Bulb', 'Bulb'],
  ];

  it('labels the 53 choices of a D600 as standard shutter speeds', () => {
    expect(table).toHaveLength(53);
    for (const [choice, label] of table) expect(formatShutterChoice(choice)).toBe(label);
  });

  it('gives every choice of the D600 a label of its own', () => {
    const labels = table.map(([choice]) => formatShutterChoice(choice));
    expect(new Set(labels).size).toBe(labels.length);
  });

  it('leaves text that is not a time as it is, and writes times over 30 s plainly', () => {
    expect(formatShutterChoice('Bulb')).toBe('Bulb');
    expect(formatShutterChoice('1/100')).toBe('1/100');
    expect(formatShutterChoice('')).toBe('');
    expect(formatShutterChoice('60.0000s')).toBe('60 s');
  });
});
