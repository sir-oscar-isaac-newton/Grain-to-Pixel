// Pure helpers for the export presets of the export dialog (U-21).
import type { ExportOptions, ExportPreset } from '../../shared/ipc';

/** Names are compared without surrounding spaces and without case, as the app's settings do. */
const keyOf = (name: string) => name.trim().toLowerCase();

/** The preset called `name` (spaces and case ignored), or undefined. */
export function findPreset(
  presets: readonly ExportPreset[],
  name: string | null | undefined,
): ExportPreset | undefined {
  if (name == null) return undefined;
  const key = keyOf(name);
  return presets.find((p) => keyOf(p.name) === key);
}

/**
 * Whether two sets of options are the same. The JPEG quality only counts when the format is JPEG
 * (it is not shown for TIFF), a missing size is the same as none (full size), and `ignoreFolder`
 * leaves the folder out.
 */
export function sameOptions(
  a: ExportOptions,
  b: ExportOptions,
  { ignoreFolder = false }: { ignoreFolder?: boolean } = {},
): boolean {
  return (
    a.format === b.format &&
    a.space === b.space &&
    (a.format !== 'jpeg' || a.jpeg_quality === b.jpeg_quality) &&
    a.template === b.template &&
    a.write_metadata === b.write_metadata &&
    (a.long_edge ?? null) === (b.long_edge ?? null) &&
    (ignoreFolder || a.dest_dir === b.dest_dir)
  );
}

/**
 * Whether the options shown differ from the preset: a preset without a folder says nothing about
 * the folder, so the folder is not compared then.
 */
export function isModified(options: ExportOptions, preset: ExportPreset): boolean {
  return !sameOptions(options, preset.options, { ignoreFolder: preset.options.dest_dir === '' });
}

/**
 * The options after choosing a preset: all of the preset's, except that a preset without a folder
 * keeps the folder that is already chosen.
 */
export function applyPreset(current: ExportOptions, preset: ExportPreset): ExportOptions {
  return {
    ...preset.options,
    long_edge: preset.options.long_edge ?? null,
    dest_dir: preset.options.dest_dir === '' ? current.dest_dir : preset.options.dest_dir,
  };
}

/** The translation key of what is wrong with a new preset name, or null. */
export function nameProblem(
  name: string,
  presets: readonly ExportPreset[],
): 'export.presets.errors.noName' | 'export.presets.errors.exists' | null {
  if (name.trim() === '') return 'export.presets.errors.noName';
  if (findPreset(presets, name)) return 'export.presets.errors.exists';
  return null;
}
