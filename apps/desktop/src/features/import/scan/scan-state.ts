import type { StepId } from '../guide-steps';
import type { StepState } from '../capture/steps/step-state';
import type { Scan } from './ScanProvider';

const NONE: StepState = { mark: null, canPass: true };
const OPEN: StepState = { mark: null, canPass: false };
const DONE: StepState = { mark: 'done', canPass: true };

/**
 * The state of one of the scanner steps. **Scanner** is done when a scanner is found; **Frames**
 * when the first and last frame are a valid range, or when every frame of the roll has been scanned
 * (there is nothing left to choose, and Finish is still allowed); the other steps have no state.
 */
export function scanStepState(id: StepId, scan: Scan): StepState {
  switch (id) {
    case 'scanner':
      return scan.device ? DONE : OPEN;
    case 'frames':
      return scan.valid || scan.complete ? DONE : OPEN;
    default:
      return NONE;
  }
}
