import { Slider } from '@gtp/ui';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { captureProfileSet, type CaptureProfile } from '../../../shared/ipc';
import { messageOf } from '../../library/library-store';
import styles from '../import.module.css';

const CHANNELS = ['red', 'green', 'blue'] as const;

type Levels = [number, number, number];

interface LevelSlidersProps {
  profile: CaptureProfile;
  /** Called with the profile the backend saved. */
  onSaved: (profile: CaptureProfile) => void;
  disabled: boolean;
}

/**
 * One slider per colour for the LED level of that colour's exposure. A change is saved when the
 * thumb is let go; the sliders never switch the light on (a test shot does).
 */
export function LevelSliders({ profile, onSaved, disabled }: LevelSlidersProps) {
  const { t } = useTranslation();
  const [levels, setLevels] = useState<Levels>(profile.levels);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setLevels(profile.levels), [profile.levels]);

  const change = (index: number, value: number, dragging: boolean) => {
    const next = levels.map((v, i) => (i === index ? value : v)) as Levels;
    setLevels(next);
    if (dragging) return;
    setError(null);
    captureProfileSet({ levels: next }).then(onSaved, (e: unknown) => {
      setError(messageOf(e));
      setLevels(profile.levels);
    });
  };

  return (
    <div className={styles.captureLevels}>
      {CHANNELS.map((channel, i) => (
        <Slider
          key={channel}
          label={t(`import.capture.levels.${channel}`)}
          value={levels[i] ?? 0}
          min={0}
          max={255}
          step={1}
          disabled={disabled}
          onChange={(value, dragging) => change(i, value, dragging)}
        />
      ))}
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
