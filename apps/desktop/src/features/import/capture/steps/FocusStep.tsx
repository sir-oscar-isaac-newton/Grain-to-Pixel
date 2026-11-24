import { useTranslation } from 'react-i18next';
import styles from '../../import.module.css';
import { useCaptureRig } from '../CaptureRig';
import { FocusCheck } from '../FocusCheck';
import { RigLiveView } from './RigLiveView';
import { StepSection } from './StepSection';

/** Step "Focus and framing": a film frame in the carrier, live view and the focus check. */
export function FocusStep() {
  const { t } = useTranslation();
  const rig = useCaptureRig();
  return (
    <StepSection id="import-focus" heading={t('import.steps.focus.heading')}>
      <p className={styles.muted}>{t('import.steps.focus.intro')}</p>
      <RigLiveView />
      {rig.levels && (
        <FocusCheck
          disabled={rig.running || !rig.usable || rig.settingsChanging}
          levels={rig.levels}
          onBusyChange={rig.setFocusing}
        />
      )}
    </StepSection>
  );
}
