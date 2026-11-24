import { describe, expect, it } from 'vitest';
import type { FilmType, ImportSource } from '../../shared/ipc';
import { isStepId, steps } from './guide-steps';

const FILM_TYPES: FilmType[] = ['color_negative', 'bw_silver', 'bw_chromogenic'];

describe('guide steps', () => {
  it('starts every source with the roll and ends it with its own page', () => {
    expect(steps('camera', 'color_negative')).toEqual(['roll', 'capture']);
    expect(steps('scanner', 'color_negative')).toEqual(['roll', 'scan']);
    expect(steps('files', 'color_negative')).toEqual(['roll', 'files']);
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
    expect(isStepId('flat')).toBe(false);
  });
});
