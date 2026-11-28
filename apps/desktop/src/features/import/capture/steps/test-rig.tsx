import { useLibraryStore } from '../../../library/library-store';
import { useLibraryEvents } from '../../../library/use-library-events';
import { CaptureRigProvider, useCaptureRig } from '../CaptureRig';
import { NextFrameButton } from '../NextFrameButton';
import { StatusBar } from '../StatusBar';
import { CaptureStep } from './CaptureStep';
import { FlatFieldStep } from './FlatFieldStep';
import { FocusStep } from './FocusStep';
import { LevelsStep } from './LevelsStep';
import { SetupStep } from './SetupStep';

/**
 * The camera steps, and two parts of them that tests can show on their own: `status` (the light
 * and camera status bar of Set up) and `next` (the Next frame button of Capture).
 */
export type CameraStep = 'setup' | 'focus' | 'flatfield' | 'levels' | 'capture' | 'status' | 'next';

/**
 * The parts most tests show together. The guide shows one step at a time, and each of Set up, Flat
 * field and Light levels has the camera's settings, Focus, Light levels and Capture a live view,
 * so this set takes the parts only: the status bar, Light levels and the Next frame button.
 */
export const DEFAULT_STEPS: CameraStep[] = ['status', 'levels', 'next'];

function Status() {
  const rig = useCaptureRig();
  return (
    <StatusBar
      light={rig.light}
      warnC={rig.profile?.temp_warn_c ?? null}
      cameras={rig.cameras}
      busy={rig.connecting || rig.running}
      error={rig.error}
      onConnect={() => void rig.connect()}
      onRefreshCameras={() => {
        rig.clearError();
        void rig.refreshCameras();
      }}
    />
  );
}

function Next() {
  const rig = useCaptureRig();
  return (
    <NextFrameButton
      ready={rig.usable && !rig.settingsChanging && !rig.running}
      roll={rig.roll}
      levels={rig.levels}
      onBusyChange={rig.setCapturing}
    />
  );
}

/**
 * Tests only: camera steps inside a rig, on the library's selected roll (else its first), with
 * the job events running as they do in the app. Several steps show at once so a test can reach
 * what it needs without walking the guide; keep out of one set the steps that share a part.
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
          case 'status':
            return <Status key={step} />;
          case 'next':
            return <Next key={step} />;
        }
      })}
    </CaptureRigProvider>
  );
}
