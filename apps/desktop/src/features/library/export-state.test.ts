import { describe, expect, it } from 'vitest';
import { exportState } from './export-state';

describe('exportState', () => {
  it('is never for a frame without an export, edited or not', () => {
    expect(exportState({ edited_at: null, exported_at: null })).toBe('never');
    expect(exportState({ edited_at: '2026-11-03T10:00:00Z', exported_at: null })).toBe('never');
  });

  it('is current for a frame exported and never edited', () => {
    expect(exportState({ edited_at: null, exported_at: '2026-11-03T10:00:00Z' })).toBe('current');
  });

  it('is current when the edit came before the export or in the same second', () => {
    expect(
      exportState({ edited_at: '2026-11-02T10:00:00Z', exported_at: '2026-11-03T10:00:00Z' }),
    ).toBe('current');
    expect(
      exportState({ edited_at: '2026-11-03T10:00:00Z', exported_at: '2026-11-03T10:00:00Z' }),
    ).toBe('current');
  });

  it('is stale when the edit came after the export', () => {
    expect(
      exportState({ edited_at: '2026-11-03T10:00:01Z', exported_at: '2026-11-03T10:00:00Z' }),
    ).toBe('stale');
    expect(
      exportState({ edited_at: '2026-12-01T00:00:00Z', exported_at: '2026-11-30T23:59:59Z' }),
    ).toBe('stale');
  });
});
