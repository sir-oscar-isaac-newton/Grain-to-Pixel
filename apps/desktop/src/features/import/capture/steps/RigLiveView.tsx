import { useTranslation } from 'react-i18next';
import styles from '../../import.module.css';
import { LiveView } from '../LiveView';
import { useCaptureRig } from '../CaptureRig';

interface RigLiveViewProps {
  /** True where live view must come back after a capture (the Capture step). */
  resumeAfterBlock?: boolean;
}

/** The live view of the rig's camera (where the camera backend has one), with its heading. */
export function RigLiveView({ resumeAfterBlock = false }: RigLiveViewProps) {
  const { t } = useTranslation();
  const rig = useCaptureRig();
  const { profile, cameras, light } = rig;
  if (!profile) return null;
  return (
    <div className={styles.captureLive}>
      <h4 className={styles.subheading}>{t('import.capture.liveView.heading')}</h4>
      {profile.camera_backend === 'gphoto' ? (
        cameras.length > 0 && (
          <LiveView
            calibrationArea={rig.calibrationArea}
            onCalibrationAreaChange={rig.setCalibrationArea}
            stopRef={rig.liveStop}
            levels={rig.levels}
            lightLost={!light.connected || light.status?.tripped === true}
            blocked={
              rig.running || rig.shotBusy || rig.calibrating || rig.capturing || rig.focusing
            }
            settingsChanging={rig.settingsChanging}
            resumeAfterBlock={resumeAfterBlock}
          />
        )
      ) : (
        <p className={styles.muted}>{t('import.capture.liveView.macOnly')}</p>
      )}
    </div>
  );
}
