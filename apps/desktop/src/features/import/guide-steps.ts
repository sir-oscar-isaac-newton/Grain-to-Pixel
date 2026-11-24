import type { FilmType, ImportSource } from '../../shared/ipc';

/** The ids of the guide's steps; the settings keep them as the last step of a roll. */
export type StepId =
  | 'roll'
  | 'setup'
  | 'focus'
  | 'flatfield'
  | 'levels'
  | 'capture'
  | 'scanner'
  | 'frames'
  | 'scan-settings'
  | 'scan'
  | 'files'
  | 'import';

export const STEP_IDS: readonly StepId[] = [
  'roll',
  'setup',
  'focus',
  'flatfield',
  'levels',
  'capture',
  'scanner',
  'frames',
  'scan-settings',
  'scan',
  'files',
  'import',
];

/** Whether text is the id of a step. */
export const isStepId = (id: string): id is StepId => (STEP_IDS as readonly string[]).includes(id);

/**
 * The steps of the guide for a roll's source, in order. This is the one table of steps: U-36 split
 * the camera capture into steps, U-37 the scanner and the files (their last steps keep the ids
 * 'scan' and 'files' of the one page each had before, so a saved place still resumes there), and
 * H-25 makes the steps depend on the film type, which is why it is already a parameter.
 */
export function steps(source: ImportSource, filmType: FilmType): StepId[] {
  void filmType;
  switch (source) {
    case 'camera':
      // Black-and-white silver changes with H-25's one-shot capture.
      return ['roll', 'setup', 'focus', 'flatfield', 'levels', 'capture'];
    case 'scanner':
      return ['roll', 'scanner', 'frames', 'scan-settings', 'scan'];
    case 'files':
      return ['roll', 'files', 'import'];
  }
}
