import { Slider } from '@gtp/ui';
import { useTranslation } from 'react-i18next';
import type { Levels } from '../../../shared/ipc';
import styles from '../import.module.css';

const CHANNELS = ['red', 'green', 'blue'] as const;

interface LevelSlidersProps {
  /** The working levels: used by the test shot and Next frame, kept only by an explicit save. */
  levels: Levels;
  onChange: (levels: Levels) => void;
  disabled: boolean;
}

/**
 * One slider per colour for the LED level of that colour's exposure. They change the working
 * levels only; nothing is saved or sent, and the light stays off (a test shot lights it).
 */
export function LevelSliders({ levels, onChange, disabled }: LevelSlidersProps) {
  const { t } = useTranslation();
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
          onChange={(value) => onChange(levels.map((v, j) => (j === i ? value : v)) as Levels)}
        />
      ))}
    </div>
  );
}
