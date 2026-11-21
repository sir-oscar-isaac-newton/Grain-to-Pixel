import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { onCaptureStep, type CaptureStep } from '../../../shared/ipc';
import styles from '../import.module.css';

const CHANNELS = ['red', 'green', 'blue'] as const;

export type Steps = Partial<Record<CaptureStep['channel'], CaptureStep['state']>>;

/** Follows the steps of a running capture (frame or flat field); the second value forgets them. */
export function useCaptureSteps(): [Steps, () => void] {
  const [steps, setSteps] = useState<Steps>({});
  useEffect(() => {
    let alive = true;
    let stop: (() => void) | undefined;
    onCaptureStep((step) => setSteps((all) => ({ ...all, [step.channel]: step.state }))).then(
      (stopListening) => {
        if (alive) stop = stopListening;
        else stopListening();
      },
      () => {
        // No backend (tests, plain browser): there are no steps to follow.
      },
    );
    return () => {
      alive = false;
      stop?.();
    };
  }, []);
  const reset = useCallback(() => setSteps({}), []);
  return [steps, reset];
}

/** The red, green and blue exposures of a capture with what each is doing now. */
export function CaptureSteps({ steps }: { steps: Steps }) {
  const { t } = useTranslation();
  return (
    <ol aria-label={t('import.capture.next.steps')} className={styles.captureSteps}>
      {CHANNELS.map((channel) => (
        <li key={channel}>
          {t('import.capture.next.step', {
            channel: t(`import.channel.${channel}`),
            state: t(`import.capture.next.state.${steps[channel] ?? 'waiting'}`),
          })}
        </li>
      ))}
    </ol>
  );
}
