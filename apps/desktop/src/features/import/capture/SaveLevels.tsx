import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  captureLevelsGet,
  captureLevelsSave,
  type CaptureLevels,
  type ExposureInfo,
  type Levels,
} from '../../../shared/ipc';
import { messageOf } from '../../library/library-store';
import styles from '../import.module.css';
import { exposureDiffers } from './exposure';

/** The test shot the working levels were last tried with. */
export interface LastShot {
  exposure: ExposureInfo;
}

interface SaveLevelsProps {
  rollId: string | null;
  levels: Levels;
  /** What the roll's levels are now (saved), to compare the test shot's camera settings with. */
  saved: CaptureLevels | null;
  /** The roll's film stock, trimmed; null when it has none. */
  stock: string | null;
  /** The last test shot, only while no slider has moved since; null otherwise. */
  shot: LastShot | null;
  disabled: boolean;
  /** Called with the levels as they resolve for the roll after a save. */
  onSaved: (saved: CaptureLevels) => void;
}

/**
 * Saving the working levels, as the global default or for the roll's film stock. Only after a test
 * shot at these levels, so that what is saved was seen, with the camera settings of that shot.
 */
export function SaveLevels({
  rollId,
  levels,
  saved,
  stock,
  shot,
  disabled,
  onSaved,
}: SaveLevelsProps) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // A message about an earlier save is stale once the levels move.
  useEffect(() => setDone(false), [levels]);

  const canSave = shot !== null && !disabled && !busy;
  const differs = shot !== null && exposureDiffers(shot.exposure, saved?.exposure ?? null);

  const save = async (scope: 'default' | { stock: string }) => {
    if (!shot) return;
    setBusy(true);
    setDone(false);
    setError(null);
    try {
      await captureLevelsSave(scope, levels, shot.exposure);
      if (rollId) onSaved(await captureLevelsGet(rollId));
      setDone(true);
    } catch (e) {
      setError(messageOf(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={styles.captureTest}>
      <div className={styles.buttons}>
        <button type="button" disabled={!canSave} onClick={() => void save('default')}>
          {t('import.capture.levels.saveDefault')}
        </button>
        {stock && (
          <button type="button" disabled={!canSave} onClick={() => void save({ stock })}>
            {t('import.capture.levels.saveStock', { stock })}
          </button>
        )}
      </div>
      {shot === null && <p className={styles.muted}>{t('import.capture.levels.needTest')}</p>}
      {differs && <p role="status">{t('import.capture.levels.exposureDiffers')}</p>}
      {done && <p role="status">{t('import.capture.levels.saved')}</p>}
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
