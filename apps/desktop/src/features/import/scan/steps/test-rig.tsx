import { useLibraryStore } from '../../../library/library-store';
import { useLibraryEvents } from '../../../library/use-library-events';
import { ScanProvider } from '../ScanProvider';
import { FramesStep } from './FramesStep';
import { ScanSettingsStep } from './ScanSettingsStep';
import { ScanStep } from './ScanStep';
import { ScannerStep } from './ScannerStep';

/**
 * Tests only: the scanner steps inside a provider, on the library's selected roll (else its first),
 * with the job events running as they do in the app. All four show at once so a test can reach what
 * it needs without walking the guide.
 */
export function ScanSteps() {
  useLibraryEvents(true);
  const rollId = useLibraryStore((s) => s.rollId ?? s.rolls[0]?.id ?? null);
  return (
    <ScanProvider rollId={rollId}>
      <ScannerStep />
      <FramesStep />
      <ScanSettingsStep />
      <ScanStep />
    </ScanProvider>
  );
}
