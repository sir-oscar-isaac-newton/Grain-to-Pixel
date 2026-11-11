import { describe, expect, it } from 'vitest';
import type { DevelopPreset } from '../../shared/ipc';
import { makeRecipe } from './fake-ipc';
import {
  DEFAULT_PRESET_PARTS,
  PRESET_PARTS,
  findDevelopPreset,
  orderedParts,
  presetNameProblem,
} from './develop-presets';

const preset = (name: string): DevelopPreset => ({
  name,
  fields: ['Tone'],
  recipe: makeRecipe('p') as DevelopPreset['recipe'],
});

describe('develop presets helpers', () => {
  const presets = [preset('Warm'), preset('Cool')];

  it('finds a name ignoring case and surrounding spaces', () => {
    expect(findDevelopPreset(presets, ' warm ')).toBe(presets[0]);
    expect(findDevelopPreset(presets, 'Neutral')).toBeUndefined();
  });

  it('refuses an empty name and a name in use, without regard to case', () => {
    expect(presetNameProblem('  ', presets)).toBe('develop.presets.errors.noName');
    expect(presetNameProblem('COOL', presets)).toBe('develop.presets.errors.exists');
    expect(presetNameProblem('Neutral', presets)).toBeNull();
  });

  it('lets a preset being renamed keep its name or change its case', () => {
    expect(presetNameProblem('warm', presets, presets[0])).toBeNull();
    expect(presetNameProblem('Cool', presets, presets[0])).toBe('develop.presets.errors.exists');
  });

  it('lists the parts in the shown order, and ticks colour, the colour mixer, split toning and tone by default', () => {
    expect(orderedParts(['Output', 'Tone', 'Color'])).toEqual(['Color', 'Tone', 'Output']);
    expect(DEFAULT_PRESET_PARTS).toEqual(['Color', 'Mixer', 'SplitTone', 'Tone']);
    expect(PRESET_PARTS).toHaveLength(10);
    expect(PRESET_PARTS.indexOf('Noise')).toBe(PRESET_PARTS.indexOf('Dust') + 1);
    expect(DEFAULT_PRESET_PARTS).not.toContain('Noise');
    expect(PRESET_PARTS.indexOf('Mixer')).toBe(PRESET_PARTS.indexOf('Color') + 1);
  });
});
