import { describe, expect, it } from 'vitest';
import type { ExportOptions, ExportPreset } from '../../shared/ipc';
import { applyPreset, findPreset, isModified, nameProblem, sameOptions } from './export-presets';

const options = (over: Partial<ExportOptions> = {}): ExportOptions => ({
  format: 'jpeg',
  space: 'srgb',
  jpeg_quality: 90,
  dest_dir: '/out',
  template: '{roll}_{index:03}',
  ...over,
});

const preset = (name: string, over: Partial<ExportOptions> = {}): ExportPreset => ({
  name,
  options: options(over),
});

describe('findPreset', () => {
  const presets = [preset('Share'), preset('Archive')];

  it('finds a name ignoring case and surrounding spaces', () => {
    expect(findPreset(presets, ' share ')).toBe(presets[0]);
    expect(findPreset(presets, 'ARCHIVE')).toBe(presets[1]);
  });

  it('finds nothing for an unknown, null or missing name', () => {
    expect(findPreset(presets, 'Web')).toBeUndefined();
    expect(findPreset(presets, null)).toBeUndefined();
    expect(findPreset(presets, undefined)).toBeUndefined();
    expect(findPreset([], 'Share')).toBeUndefined();
  });
});

describe('sameOptions', () => {
  it('compares every option', () => {
    expect(sameOptions(options(), options())).toBe(true);
    for (const change of [
      { format: 'tiff16' },
      { space: 'prophoto' },
      { jpeg_quality: 80 },
      { dest_dir: '/other' },
      { template: 'x' },
    ] as const) {
      expect(sameOptions(options(), options(change))).toBe(false);
    }
  });

  it('ignores the quality for TIFF, where it is not shown', () => {
    expect(
      sameOptions(options({ format: 'tiff16' }), options({ format: 'tiff16', jpeg_quality: 50 })),
    ).toBe(true);
  });

  it('can leave the folder out', () => {
    expect(sameOptions(options(), options({ dest_dir: '' }), { ignoreFolder: true })).toBe(true);
  });
});

describe('isModified', () => {
  it('compares the folder only when the preset has one', () => {
    expect(isModified(options({ dest_dir: '/elsewhere' }), preset('A', { dest_dir: '' }))).toBe(
      false,
    );
    expect(isModified(options({ dest_dir: '/elsewhere' }), preset('A'))).toBe(true);
    expect(isModified(options({ space: 'adobe_rgb' }), preset('A', { dest_dir: '' }))).toBe(true);
  });
});

describe('applyPreset', () => {
  it('fills every option from the preset', () => {
    const chosen = preset('A', { format: 'tiff16', space: 'prophoto', template: 'T' });
    expect(applyPreset(options({ dest_dir: '/old' }), chosen)).toEqual(chosen.options);
  });

  it('keeps the current folder when the preset has none', () => {
    const chosen = preset('A', { dest_dir: '', space: 'prophoto' });
    expect(applyPreset(options({ dest_dir: '/current' }), chosen)).toEqual({
      ...chosen.options,
      dest_dir: '/current',
    });
  });
});

describe('nameProblem', () => {
  const presets = [preset('Share')];

  it('refuses an empty name and one that is only spaces', () => {
    expect(nameProblem('', presets)).toBe('export.presets.errors.noName');
    expect(nameProblem('   ', presets)).toBe('export.presets.errors.noName');
  });

  it('refuses a name in use, ignoring case and spaces', () => {
    expect(nameProblem('share', presets)).toBe('export.presets.errors.exists');
    expect(nameProblem(' SHARE ', presets)).toBe('export.presets.errors.exists');
  });

  it('accepts a new name', () => {
    expect(nameProblem('Web', presets)).toBeNull();
    expect(nameProblem('Web', [])).toBeNull();
  });
});
