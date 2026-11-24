import type { ExposureInfo } from '../../../../shared/ipc';
import { apertureDiffers, cameraDiffers } from '../exposure';
import { sameLevels, type CaptureRig } from '../CaptureRig';
import type { StepId } from '../../guide-steps';

/** How a step shows in the step bar: done, skipped by the user, or in need of another look. */
export type StepMark = 'done' | 'skipped' | 'attention';

/** What the guide knows of a step: its mark, and whether Next may pass it. */
export interface StepState {
  mark: StepMark | null;
  /** False while the step still has to be done (or looked at again). */
  canPass: boolean;
}

const NONE: StepState = { mark: null, canPass: true };
const OPEN: StepState = { mark: null, canPass: false };

/** Whether the flat field of the roll was taken at the aperture the camera has now. */
export function flatFieldState(rig: CaptureRig): StepState {
  const roll = rig.roll;
  const has = roll?.has_flat_field === true;
  const differs = has && apertureDiffers(rig.camera, roll?.flat_field_f_number);
  if (has && !differs) return { mark: 'done', canPass: true };
  if (rig.flatFieldSkipped) return { mark: 'skipped', canPass: true };
  if (has) return { mark: 'attention', canPass: false };
  return OPEN;
}

/** What the levels stand on now: the exposure they were saved, calibrated or chosen with. */
export function levelsBasis(rig: CaptureRig): { exposure: ExposureInfo | null } | null {
  const { levels, saved, calibrated } = rig;
  if (!levels || !saved) return null;
  if (saved.source === 'stock' && sameLevels(levels, saved.levels)) {
    return { exposure: saved.exposure };
  }
  if (calibrated && sameLevels(levels, calibrated.levels)) {
    return { exposure: calibrated.exposure };
  }
  if (rig.globalLevels && saved.source === 'default' && sameLevels(levels, saved.levels)) {
    return { exposure: saved.exposure };
  }
  return null;
}

/** Whether the levels are settled, and with the camera as it is now. */
export function levelsState(rig: CaptureRig): StepState {
  const basis = levelsBasis(rig);
  if (!basis) return OPEN;
  if (cameraDiffers(rig.camera, basis.exposure)) return { mark: 'attention', canPass: false };
  return { mark: 'done', canPass: true };
}

/** The state of one of the camera steps; the other steps of the guide have none of their own. */
export function cameraStepState(id: StepId, rig: CaptureRig): StepState {
  switch (id) {
    case 'setup':
      return rig.usable ? { mark: 'done', canPass: true } : OPEN;
    case 'flatfield':
      return flatFieldState(rig);
    case 'levels':
      return levelsState(rig);
    default:
      return NONE;
  }
}
