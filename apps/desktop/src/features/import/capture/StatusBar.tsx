import { StatusBadge, type BadgeTone } from '@gtp/ui';
import { useTranslation } from 'react-i18next';
import type { CameraInfo, LightStatus } from '../../../shared/ipc';
import styles from '../import.module.css';

/** The supply voltage under which the light is warned about (spec section 6.1), in millivolts. */
export const LOW_SUPPLY_MV = 4750;

export interface LightState {
  /** True once the light answered (now or earlier in this session). */
  connected: boolean;
  /** The last reading; null until the first one. */
  status: LightStatus | null;
}

interface StatusBarProps {
  light: LightState;
  /** The temperature above which the light counts as warm; null for no limit. */
  warnC: number | null;
  cameras: CameraInfo[];
  busy: boolean;
  /** What went wrong with the light or the camera list, in words. */
  error: string | null;
  onConnect: () => void;
  onRefreshCameras: () => void;
}

/** Whether the light is warm: above the profile's warn temperature. */
export function isWarm(temp: number | null | undefined, warnC: number | null): boolean {
  return temp !== null && temp !== undefined && warnC !== null && temp > warnC;
}

/** Badges for the light and the camera, and the buttons that connect them. */
export function StatusBar({
  light,
  warnC,
  cameras,
  busy,
  error,
  onConnect,
  onRefreshCameras,
}: StatusBarProps) {
  const { t } = useTranslation();
  const temp = light.status?.temp_c ?? null;
  const tripped = light.status?.tripped === true;
  const warm = isWarm(temp, warnC);
  const lowSupply = (light.status?.vbus_mv ?? LOW_SUPPLY_MV) < LOW_SUPPLY_MV;

  let tone: BadgeTone = 'neutral';
  let text = t('import.capture.light.off');
  if (tripped) {
    tone = 'error';
    text = t('import.capture.light.tripped');
  } else if (light.connected) {
    tone = warm ? 'warn' : 'ok';
    text =
      temp === null
        ? t('import.capture.light.connected')
        : t(warm ? 'import.capture.light.warm' : 'import.capture.light.connectedTemp', {
            temp: Math.round(temp),
          });
  }

  return (
    <div className={styles.captureStatus}>
      <div className={styles.row}>
        <StatusBadge tone={tone}>{text}</StatusBadge>
        <button type="button" disabled={busy} onClick={onConnect}>
          {light.connected || tripped
            ? t('import.capture.light.reconnect')
            : t('import.capture.light.connect')}
        </button>
        <StatusBadge tone={cameras.length > 0 ? 'ok' : 'warn'}>
          {cameras.length > 0
            ? t('import.capture.camera.found', { name: cameras.map((c) => c.name).join(', ') })
            : t('import.capture.camera.none')}
        </StatusBadge>
        <button type="button" disabled={busy} onClick={onRefreshCameras}>
          {t('import.capture.camera.refresh')}
        </button>
      </div>
      {warm && !tripped && <p role="status">{t('import.capture.light.warmHint')}</p>}
      {light.connected && lowSupply && (
        <p role="status">
          {t('import.capture.light.lowSupply', { mv: light.status?.vbus_mv ?? 0 })}
        </p>
      )}
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
