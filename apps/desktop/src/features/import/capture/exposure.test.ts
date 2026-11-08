import { describe, expect, it } from 'vitest';
import type { ExposureInfo } from '../../../shared/ipc';
import { exposureDiffers } from './exposure';

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
