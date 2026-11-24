import { useTranslation } from 'react-i18next';
import styles from '../../import.module.css';
import { CameraSettings } from '../CameraSettings';
import { useCaptureRig } from '../CaptureRig';
import { formatExposure } from '../exposure';
import { StatusBar } from '../StatusBar';
import { StepSection } from './StepSection';

const CHECKS = ['mode', 'focus', 'aperture', 'cable'] as const;

/** Step "Set up": connect the light and the camera, check the camera's settings and the rig. */
export function SetupStep() {
  const { t } = useTranslation();
  const rig = useCaptureRig();
  const { saved } = rig;
  const savedExposure = formatExposure(saved?.exposure ?? null);
  return (
    <StepSection id="import-setup" heading={t('import.steps.setup.heading')}>
      <p className={styles.muted}>{t('import.steps.setup.intro')}</p>
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
      <CameraSettings
        cameras={rig.cameras}
        running={rig.running}
        onChanging={rig.setSettingsChanging}
        onValues={rig.onCameraSettings}
      />
      {savedExposure && (
        <p className={styles.muted}>
          {saved?.source === 'stock'
            ? t('import.capture.settings.savedStock', {
                stock: saved.stock ?? '',
                exposure: savedExposure,
              })
            : t('import.capture.settings.savedDefault', { exposure: savedExposure })}
        </p>
      )}
      <h4 className={styles.subheading}>{t('import.steps.setup.checklist')}</h4>
      <ul className={styles.checklist}>
        {CHECKS.map((check) => (
          <li key={check}>{t(`import.steps.setup.check.${check}`)}</li>
        ))}
      </ul>
    </StepSection>
  );
}
