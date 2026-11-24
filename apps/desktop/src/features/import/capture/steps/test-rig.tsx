import { useLibraryStore } from '../../../library/library-store';
import { useLibraryEvents } from '../../../library/use-library-events';
import { CaptureRigProvider } from '../CaptureRig';
import { CaptureStep } from './CaptureStep';
import { FlatFieldStep } from './FlatFieldStep';
import { FocusStep } from './FocusStep';
import { LevelsStep } from './LevelsStep';
import { SetupStep } from './SetupStep';

export type CameraStep = 'setup' | 'focus' | 'flatfield' | 'levels' | 'capture';

/** The steps most tests show together: they hold no live view twice (Focus and Light levels each have one). */
export const DEFAULT_STEPS: CameraStep[] = ['setup', 'levels', 'capture', 'flatfield'];

/**
 * Tests only: camera steps inside a rig, on the library's selected roll (else its first), with
 * the job events running as they do in the app. Several steps show at once so a test can reach
 * what it needs without walking the guide.
 */
export function RigSteps({ steps = DEFAULT_STEPS }: { steps?: CameraStep[] }) {
  useLibraryEvents(true);
  const rollId = useLibraryStore((s) => s.rollId ?? s.rolls[0]?.id ?? null);
  return (
    <CaptureRigProvider rollId={rollId}>
      {steps.map((step) => {
        switch (step) {
          case 'setup':
            return <SetupStep key={step} />;
          case 'focus':
            return <FocusStep key={step} />;
          case 'flatfield':
            return <FlatFieldStep key={step} onSkipped={() => undefined} />;
          case 'levels':
            return <LevelsStep key={step} />;
          case 'capture':
            return <CaptureStep key={step} />;
        }
      })}
    </CaptureRigProvider>
  );
}
