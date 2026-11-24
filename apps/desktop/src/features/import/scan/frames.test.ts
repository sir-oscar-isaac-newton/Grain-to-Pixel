import { describe, expect, it } from 'vitest';
import { frameProblems } from './frames';

describe('frameProblems', () => {
  it('wants whole numbers from 1 to 40, the last not before the first', () => {
    expect(frameProblems('1', '40')).toEqual({ first: undefined, last: undefined });
    expect(frameProblems('7', '7')).toEqual({ first: undefined, last: undefined });
    for (const bad of ['', '0', '41', '2.5', '-1', 'x', ' ']) {
      expect(frameProblems(bad, '10').first, bad).toBe('import.scanner.frameRange');
      expect(frameProblems('1', bad).last, bad).toBe('import.scanner.frameRange');
    }
    expect(frameProblems('10', '9').last).toBe('import.scanner.frameOrder');
    expect(frameProblems('x', '9').last).toBeUndefined();
  });
});
