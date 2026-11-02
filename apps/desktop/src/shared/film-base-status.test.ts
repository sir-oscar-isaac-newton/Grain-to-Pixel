import { describe, expect, it } from 'vitest';
import { baseStatus, baseStatusText } from './film-base-status';

describe('baseStatus', () => {
  it('tells none, picked by hand, automatic and a guess apart', () => {
    expect(baseStatus(false, null)).toEqual({ kind: 'none' });
    expect(baseStatus(false, 0.9)).toEqual({ kind: 'none' });
    expect(baseStatus(true, null)).toEqual({ kind: 'hand' });
    expect(baseStatus(true, 0.64)).toEqual({ kind: 'auto', percent: 64 });
    expect(baseStatus(true, 0.5)).toEqual({ kind: 'auto', percent: 50 });
    expect(baseStatus(true, 0.49)).toEqual({ kind: 'guess', percent: 49 });
    expect(baseStatus(true, 0)).toEqual({ kind: 'guess', percent: 0 });
  });

  it('words each status, a guess pointing to the picker', () => {
    const t = ((key: string, values?: { percent?: number }) =>
      `${key}${values?.percent === undefined ? '' : `:${values.percent}`}`) as never;
    expect(baseStatusText({ kind: 'none' }, t)).toBe('filmBase.status.none');
    expect(baseStatusText({ kind: 'hand' }, t)).toBe('filmBase.status.hand');
    expect(baseStatusText({ kind: 'auto', percent: 64 }, t)).toBe('filmBase.status.auto:64');
    expect(baseStatusText({ kind: 'guess', percent: 12 }, t)).toBe('filmBase.status.guess:12');
  });
});
