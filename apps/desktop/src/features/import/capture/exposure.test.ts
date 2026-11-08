import { describe, expect, it } from 'vitest';
import type { ExposureInfo } from '../../../shared/ipc';
import { exposureDiffers, formatExposure } from './exposure';

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
