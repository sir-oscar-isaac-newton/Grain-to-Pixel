import type { Settings } from '../../shared/ipc';

/** The runtime feature flags (spec section 10), in the order they are listed. */
export const FLAG_NAMES = [
  'dust_removal',
  'triplet_import',
  'guided_capture',
  'dev_overlay',
  'experimental_auto_levels',
] as const;

export type FlagName = (typeof FLAG_NAMES)[number];
export type FlagValues = Record<FlagName, boolean>;

/** The flags of some settings as plain booleans (the generated type allows missing fields). */
export function flagsOf(settings: Settings): FlagValues {
  const flags = settings.flags;
  return {
    dust_removal: flags?.dust_removal === true,
    triplet_import: flags?.triplet_import === true,
    guided_capture: flags?.guided_capture === true,
    dev_overlay: flags?.dev_overlay === true,
    experimental_auto_levels: flags?.experimental_auto_levels === true,
  };
}
