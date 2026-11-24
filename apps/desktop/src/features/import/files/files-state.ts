import type { StepId } from '../guide-steps';
import type { StepState } from '../capture/steps/step-state';
import type { Files } from './FilesProvider';

const NONE: StepState = { mark: null, canPass: true };
const OPEN: StepState = { mark: null, canPass: false };
const DONE: StepState = { mark: 'done', canPass: true };

/**
 * The state of one of the files steps. **Files** is done when at least one whole triplet is
 * confirmed (camera files) or at least one TIFF scan is chosen; **Import** has no state and ends the
 * guide.
 */
export function filesStepState(id: StepId, files: Files): StepState {
  switch (id) {
    case 'files':
      return files.count > 0 ? DONE : OPEN;
    default:
      return NONE;
  }
}
