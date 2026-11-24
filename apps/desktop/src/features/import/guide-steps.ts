import type { FilmType, ImportSource } from '../../shared/ipc';

/** The ids of the guide's steps; the settings keep them as the last step of a roll. */
export type StepId = 'roll' | 'capture' | 'scan' | 'files';

export const STEP_IDS: readonly StepId[] = ['roll', 'capture', 'scan', 'files'];

/** Whether text is the id of a step. */
export const isStepId = (id: string): id is StepId => (STEP_IDS as readonly string[]).includes(id);

/**
 * The steps of the guide for a roll's source, in order. This is the one table of steps: U-36 and
 * U-37 split the capture and scan pages into more steps, and H-25 makes them depend on the film
 * type, which is why it is already a parameter.
 */
export function steps(source: ImportSource, filmType: FilmType): StepId[] {
  void filmType;
  switch (source) {
    case 'camera':
      return ['roll', 'capture'];
    case 'scanner':
      return ['roll', 'scan'];
    case 'files':
      return ['roll', 'files'];
  }
}
