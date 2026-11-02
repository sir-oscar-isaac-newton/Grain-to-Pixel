import type { TripletCandidate, TripletFile } from '../../shared/ipc';

/** One group of files as the screen holds it: what the backend found, and whether it is confirmed. */
export interface Group {
  /** Stable while the user reorders. */
  id: number;
  files: TripletFile[];
  problem: TripletCandidate['problem'];
  confirmed: boolean;
}

/** A group is a whole triplet when it has no problem and three files. */
export function isComplete(group: Pick<Group, 'files' | 'problem'>): boolean {
  return (group.problem ?? null) === null && group.files.length === 3;
}

/** The groups of a `triplet_group` answer: the whole triplets start confirmed. */
export function toGroups(candidates: readonly TripletCandidate[]): Group[] {
  return candidates.map((candidate, id) => {
    const group = { id, files: [...candidate.files], problem: candidate.problem ?? null };
    return { ...group, confirmed: isComplete(group) };
  });
}

/**
 * The files with the one at `index` swapped with its neighbour (`delta` -1 earlier, 1 later). Moving
 * past either end changes nothing.
 */
export function moveFile<T>(files: readonly T[], index: number, delta: -1 | 1): T[] {
  const target = index + delta;
  if (index < 0 || index >= files.length || target < 0 || target >= files.length) {
    return [...files];
  }
  const here = files[index];
  const there = files[target];
  if (here === undefined || there === undefined) return [...files];
  const moved = [...files];
  moved[index] = there;
  moved[target] = here;
  return moved;
}

/** The paths to import: the confirmed whole triplets, flattened in channel order. */
export function importPaths(groups: readonly Group[]): string[] {
  return groups
    .filter((g) => g.confirmed && isComplete(g))
    .flatMap((g) => g.files.map((f) => f.path));
}

/** The name of a file in a path, for display. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}
