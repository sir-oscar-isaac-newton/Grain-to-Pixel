// Pure helpers for the Develop presets (U-28): the parts a preset can hold and its name checks.
import type { DevelopPreset, RecipeField } from '../../shared/ipc';

/** The parts a preset can hold, in the order they are shown (the order of the Apply settings dialog). */
export const PRESET_PARTS: readonly RecipeField[] = [
  'Color',
  'Mixer',
  'SplitTone',
  'Tone',
  'Geometry',
  'Base',
  'Invert',
  'Dust',
  'Noise',
  'Output',
];

/** Ticked when a preset is saved: the look of a frame, not its crop, base or levels. */
export const DEFAULT_PRESET_PARTS: readonly RecipeField[] = ['Color', 'Mixer', 'SplitTone', 'Tone'];

/** Names are compared without surrounding spaces and without case, as the app's settings do. */
const keyOf = (name: string) => name.trim().toLowerCase();

/** The preset called `name` (spaces and case ignored), or undefined. */
export function findDevelopPreset(
  presets: readonly DevelopPreset[],
  name: string,
): DevelopPreset | undefined {
  const key = keyOf(name);
  return presets.find((p) => keyOf(p.name) === key);
}

/**
 * The translation key of what is wrong with a preset name, or null. `renaming` is the preset being
 * renamed, which may keep its own name (or change only its case).
 */
export function presetNameProblem(
  name: string,
  presets: readonly DevelopPreset[],
  renaming?: DevelopPreset,
): 'develop.presets.errors.noName' | 'develop.presets.errors.exists' | null {
  if (name.trim() === '') return 'develop.presets.errors.noName';
  const same = findDevelopPreset(presets, name);
  if (same && same !== renaming) return 'develop.presets.errors.exists';
  return null;
}

/** The parts in the order they are shown, each once. */
export function orderedParts(fields: readonly RecipeField[]): RecipeField[] {
  return PRESET_PARTS.filter((part) => fields.includes(part));
}
