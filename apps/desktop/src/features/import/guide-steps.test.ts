import { describe, expect, it } from 'vitest';
import type { FilmType, ImportSource } from '../../shared/ipc';
import { isStepId, STEP_IDS, steps } from './guide-steps';

const FILM_TYPES: FilmType[] = ['color_negative', 'bw_silver', 'bw_chromogenic'];

describe('guide steps', () => {
  it('starts every source with the roll and ends it with its own pages', () => {
    expect(steps('camera', 'color_negative')).toEqual([
      'roll',
      'setup',
      'focus',
      'flatfield',
      'levels',
      'capture',
    ]);
    expect(steps('scanner', 'color_negative')).toEqual([
      'roll',
      'scanner',
      'frames',
      'scan-settings',
      'scan',
    ]);
    expect(steps('files', 'color_negative')).toEqual(['roll', 'files', 'import']);
  });

  it('keeps the ids of the pages that were one step before, so a saved place still resumes there', () => {
    expect(steps('scanner', 'color_negative').at(-1)).toBe('scan');
    expect(steps('files', 'color_negative')).toContain('files');
  });

  it('has an id for every step of every source, and no id twice', () => {
    for (const source of ['camera', 'scanner', 'files'] as ImportSource[]) {
      for (const id of steps(source, 'color_negative')) expect(STEP_IDS).toContain(id);
    }
    expect(new Set(STEP_IDS).size).toBe(STEP_IDS.length);
  });

  it('gives every film type the same steps for now', () => {
    for (const source of ['camera', 'scanner', 'files'] as ImportSource[]) {
      for (const type of FILM_TYPES) {
        expect(steps(source, type)).toEqual(steps(source, 'color_negative'));
      }
    }
  });

  it('knows which text is a step id', () => {
    expect(isStepId('capture')).toBe(true);
    expect(isStepId('flatfield')).toBe(true);
    for (const id of ['scanner', 'frames', 'scan-settings', 'scan', 'files', 'import']) {
      expect(isStepId(id), id).toBe(true);
    }
    expect(isStepId('flat')).toBe(false);
  });
});
