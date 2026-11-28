import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { flatFieldCopy, type Roll } from '../../../../shared/ipc';
import { messageOf, useLibraryStore } from '../../../library/library-store';
import { UseFlatFieldDialog } from '../../../library/UseFlatFieldDialog';
import styles from '../../import.module.css';
import { CameraSettings } from '../CameraSettings';
import { useCaptureRig } from '../CaptureRig';
import { formatAperture } from '../exposure';
import { FlatFieldCapture } from '../FlatFieldCapture';
import { flatFieldState } from './step-state';
import { StepSection } from './StepSection';

interface FlatFieldStepProps {
  /** Called after the step was skipped, to go on to the next one. */
  onSkipped: () => void;
}

/** Step "Flat field": the bare light, shot with the film out, or one of another roll, or skipped. */
export function FlatFieldStep({ onSkipped }: FlatFieldStepProps) {
  const { t } = useTranslation();
  const rig = useCaptureRig();
  const reloadRolls = useLibraryStore((s) => s.reloadRolls);
  const readOnly = useLibraryStore((s) => s.library?.read_only ?? false);
  const [reusing, setReusing] = useState<Roll | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { roll } = rig;

  const state = flatFieldState(rig);
  const aperture = formatAperture(roll?.flat_field_f_number);

  const copy = async (fromId: string, to: Roll) => {
    setError(null);
    try {
      await flatFieldCopy(fromId, to.id);
      await reloadRolls();
    } catch (e) {
      setError(t('import.steps.flatField.copyFailed', { message: messageOf(e) }));
    }
  };

  return (
    <StepSection id="import-flat-field" heading={t('import.steps.flatField.heading')}>
      <p className={styles.muted}>{t('import.steps.flatField.intro')}</p>
      <CameraSettings
        cameras={rig.cameras}
        running={rig.busy}
        onChanging={rig.setSettingsChanging}
        onValues={rig.onCameraSettings}
      />
      <FlatFieldCapture
        ready={rig.usable && !rig.settingsChanging && !rig.running}
        roll={roll}
        levels={rig.levels}
        onBusyChange={rig.setCapturing}
      />
      <div className={styles.buttons}>
        <button
          type="button"
          disabled={!roll || readOnly || rig.busy}
          onClick={() => setReusing(roll)}
        >
          {t('library.rolls.useFlatField')}
        </button>
        {state.mark !== 'done' && (
          <button
            type="button"
            disabled={rig.busy}
            onClick={() => {
              rig.setFlatFieldSkipped(true);
              onSkipped();
            }}
          >
            {t('import.steps.flatField.skip')}
          </button>
        )}
      </div>
      {error && <p role="alert">{error}</p>}
      {state.mark === 'done' && (
        <p role="status">
          {aperture
            ? t('import.steps.flatField.onRoll', { aperture })
            : t('import.steps.flatField.onRollUnknown')}
        </p>
      )}
      {state.mark === 'attention' && (
        <p role="alert">{t('import.steps.flatField.apertureChanged')}</p>
      )}
      {state.mark === 'skipped' && <p role="alert">{t('import.steps.flatField.skipped')}</p>}
      <UseFlatFieldDialog roll={reusing} onClose={() => setReusing(null)} onCopy={copy} />
    </StepSection>
  );
}
