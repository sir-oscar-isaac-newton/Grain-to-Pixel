import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  cameraSettingsGet,
  cameraSettingsSet,
  type CameraInfo,
  type CameraSetting,
  type CameraSettingKind,
} from '../../../shared/ipc';
import { messageOf } from '../../library/library-store';
import styles from '../import.module.css';

const KINDS: CameraSettingKind[] = ['iso', 'shutter_speed', 'aperture'];

interface CameraSettingsProps {
  /** The cameras found; the settings are read when there is one and again when the list changes. */
  cameras: CameraInfo[];
  /** True while a capture runs: the camera is in use, so nothing is read or changed. */
  running: boolean;
  /** Called with true while a change is on its way to the camera. */
  onChanging: (changing: boolean) => void;
}

/**
 * The camera's ISO, shutter speed and aperture, as the camera has them, with the choices it
 * allows. The camera's own dials are locked while it is tethered, so this is where they change.
 */
export function CameraSettings({ cameras, running, onChanging }: CameraSettingsProps) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<CameraSetting[] | null>(null);
  const [changing, setChanging] = useState<CameraSettingKind | null>(null);
  const [error, setError] = useState<string | null>(null);
  const found = cameras.length > 0;

  useEffect(() => {
    if (!found) {
      setSettings(null);
      setError(null);
      return;
    }
    if (running) return;
    let alive = true;
    setError(null);
    cameraSettingsGet().then(
      (read) => {
        if (alive) setSettings(read);
      },
      (e: unknown) => {
        if (!alive) return;
        setSettings(null);
        setError(messageOf(e));
      },
    );
    return () => {
      alive = false;
    };
    // A new list of cameras (also after "Look for camera again") reads the settings again.
  }, [found, cameras, running]);

  const change = (kind: CameraSettingKind, value: string) => {
    setChanging(kind);
    onChanging(true);
    setError(null);
    cameraSettingsSet(kind, value)
      .then(setSettings, (e: unknown) => setError(messageOf(e)))
      .finally(() => {
        setChanging(null);
        onChanging(false);
      });
  };

  if (!found || (settings === null && error === null)) return null;

  return (
    <fieldset className={styles.captureSettings}>
      <legend>{t('import.capture.settings.heading')}</legend>
      {settings && (
        <div className={styles.row}>
          {KINDS.map((kind) => {
            const setting = settings.find((s) => s.kind === kind) ?? null;
            return (
              <SettingField
                key={kind}
                kind={kind}
                setting={setting}
                disabled={running || changing !== null}
                onChange={(value) => change(kind, value)}
              />
            );
          })}
        </div>
      )}
      {error && <p role="alert">{error}</p>}
    </fieldset>
  );
}

interface SettingFieldProps {
  kind: CameraSettingKind;
  setting: CameraSetting | null;
  disabled: boolean;
  onChange: (value: string) => void;
}

function SettingField({ kind, setting, disabled, onChange }: SettingFieldProps) {
  const { t } = useTranslation();
  const label = t(`import.capture.settings.kind.${kind}`);
  if (!setting || setting.choices.length === 0) {
    return (
      <div className={styles.field}>
        <span>{label}</span>
        <span className={styles.muted}>
          {t(
            kind === 'aperture'
              ? 'import.capture.settings.setOnLens'
              : 'import.capture.settings.notAvailable',
          )}
        </span>
      </div>
    );
  }
  return (
    <label className={styles.field}>
      {label}
      <select
        value={setting.value ?? ''}
        disabled={disabled || !setting.writable}
        onChange={(e) => onChange(e.target.value)}
      >
        {setting.value === null && <option value="" disabled />}
        {setting.choices.map((choice) => (
          <option key={choice} value={choice}>
            {choice}
          </option>
        ))}
      </select>
    </label>
  );
}
