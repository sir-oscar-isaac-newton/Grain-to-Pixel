import { describe, expect, it } from 'vitest';
import type { TripletCandidate } from '../../shared/ipc';
import { fileName, importPaths, isComplete, moveFile, toGroups, type Group } from './triplets';

const file = (path: string) => ({ path, captured: null, channel: null });
const whole = (...names: string[]): TripletCandidate => ({
  files: names.map(file),
  problem: null,
});

describe('moveFile', () => {
  it('swaps a file with its neighbour', () => {
    expect(moveFile(['r', 'g', 'b'], 0, 1)).toEqual(['g', 'r', 'b']);
    expect(moveFile(['r', 'g', 'b'], 2, -1)).toEqual(['r', 'b', 'g']);
    expect(moveFile(['r', 'g', 'b'], 1, -1)).toEqual(['g', 'r', 'b']);
  });

  it('changes nothing past either end or for an index that is not there, and never edits its input', () => {
    const files = ['r', 'g', 'b'];
    expect(moveFile(files, 0, -1)).toEqual(['r', 'g', 'b']);
    expect(moveFile(files, 2, 1)).toEqual(['r', 'g', 'b']);
    expect(moveFile(files, 7, -1)).toEqual(['r', 'g', 'b']);
    expect(moveFile(files, -1, 1)).toEqual(['r', 'g', 'b']);
    expect(moveFile(files, 0, 1)).not.toBe(files);
    expect(files).toEqual(['r', 'g', 'b']);
  });
});

describe('groups', () => {
  it('starts the whole triplets confirmed and the others not', () => {
    const groups = toGroups([
      whole('a', 'b', 'c'),
      { files: [file('d'), file('e')], problem: { kind: 'missing_shots', missing: 1 } },
      { files: [file('f'), file('g'), file('h')], problem: { kind: 'channel_mismatch' } },
    ]);
    expect(groups.map((g) => g.confirmed)).toEqual([true, false, false]);
    expect(groups.map((g) => isComplete(g))).toEqual([true, false, false]);
    expect(groups.map((g) => g.id)).toEqual([0, 1, 2]);
  });

  it('treats a missing problem as none, and four files with no problem as not a triplet', () => {
    expect(toGroups([{ files: [file('a'), file('b'), file('c')] }])[0]?.confirmed).toBe(true);
    expect(isComplete({ files: [file('a'), file('b'), file('c'), file('d')], problem: null })).toBe(
      false,
    );
  });

  it('sends only the confirmed whole triplets, flattened in channel order', () => {
    const groups = toGroups([
      whole('a1', 'a2', 'a3'),
      whole('b1', 'b2', 'b3'),
      { files: [file('c1'), file('c2')], problem: { kind: 'missing_shots', missing: 1 } },
    ]);
    const set = (i: number, confirmed: boolean) =>
      groups.splice(i, 1, { ...(groups[i] as Group), confirmed });
    set(1, false);
    set(2, true);
    expect(importPaths(groups)).toEqual(['a1', 'a2', 'a3']);
    expect(importPaths([])).toEqual([]);
  });
});

describe('fileName', () => {
  it('is the last part of a path of either kind', () => {
    expect(fileName('C:\\films\\roll\\DSC_0001.NEF')).toBe('DSC_0001.NEF');
    expect(fileName('/films/roll/DSC_0001.NEF')).toBe('DSC_0001.NEF');
    expect(fileName('DSC_0001.NEF')).toBe('DSC_0001.NEF');
  });
});
