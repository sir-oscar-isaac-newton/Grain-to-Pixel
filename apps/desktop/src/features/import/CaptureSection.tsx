import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  cameraList,
  captureLevelsGet,
  captureProfileGet,
  jobCancel,
  lightConnect,
  lightOff,
  onLightStatus,
  settingsGet,
  type CameraInfo,
  type CaptureLevels,
  type CaptureProfile,
  type ExposureInfo,
  type Levels,
} from '../../shared/ipc';
import { flagsOf } from '../settings/flags';
import { messageOf, useLibraryStore } from '../library/library-store';
import styles from './import.module.css';
import { Calibrate } from './capture/Calibrate';
import { CameraSettings } from './capture/CameraSettings';
import { formatExposure } from './capture/exposure';
import { LevelSliders } from './capture/LevelSliders';
import { LiveView } from './capture/LiveView';
import { NextFrameButton } from './capture/NextFrameButton';
import { SaveLevels } from './capture/SaveLevels';
import { StatusBar, type LightState } from './capture/StatusBar';
import { TestShot } from './capture/TestShot';

/** A calibration shoots at the camera's own settings and reports none: it counts as the test shot. */

const sameLevels = (a: Levels, b: Levels) => a.every((v, i) => v === b[i]);

/**
 * Capturing frames with the camera and the Scanlight: status, LED levels, test shots and the Next
 * frame button. Shown only while the guided capture flag is on.
 */
export function CaptureSection() {
  const [enabled, setEnabled] = useState(false);
  useEffect(() => {
    let alive = true;
    settingsGet().then(
      (settings) => {
        if (alive) setEnabled(flagsOf(settings).guided_capture);
      },
      () => {
        // Unreadable settings leave the flag off.
      },
    );
    return () => {
      alive = false;
    };
  }, []);
  return enabled ? <Capture /> : null;
}

function Capture() {
  const { t } = useTranslation();
  const running = useLibraryStore((s) =>
    s.jobs.some((j) => (j.kind === 'capture' || j.kind === 'calibration') && j.state === 'running'),
  );
  const rolls = useLibraryStore((s) => s.rolls);
  const currentRoll = useLibraryStore((s) => s.rollId);
  const [chosenRoll, setChosenRoll] = useState<string | null>(null);
  const rollId = chosenRoll ?? currentRoll ?? rolls[0]?.id ?? null;
  const roll = rolls.find((r) => r.id === rollId) ?? null;
  const stock = roll?.film_stock?.trim() || null;

  const [light, setLight] = useState<LightState>({ connected: false, status: null });
  const [cameras, setCameras] = useState<CameraInfo[]>([]);
  const [profile, setProfile] = useState<CaptureProfile | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The levels the roll captures with once saved, the working levels the sliders hold (used by the
  // test shot and Next frame), and the camera settings of the last test shot at those levels.
  const [saved, setSaved] = useState<CaptureLevels | null>(null);
  const [levels, setLevels] = useState<Levels | null>(null);
  const [shotExposure, setShotExposure] = useState<ExposureInfo | null>(null);
  // True while a change of a camera setting is on its way to the camera.
  const [settingsChanging, setSettingsChanging] = useState(false);
  // True while a test shot, a calibration or a capture starts or runs: they take over the camera,
  // so live view stops and cannot start.
  const [shotBusy, setShotBusy] = useState(false);
  const [calibrating, setCalibrating] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const liveStop = useRef<(() => void) | null>(null);

  const refreshCameras = useCallback(async () => {
    try {
      setCameras(await cameraList());
    } catch (e) {
      setCameras([]);
      setError(messageOf(e));
    }
  }, []);

  useEffect(() => {
    void refreshCameras();
    captureProfileGet().then(setProfile, (e: unknown) => setError(messageOf(e)));
  }, [refreshCameras]);

  // The roll's levels: saved for its film stock, else the default. Choosing a roll starts from
  // them (unsaved changes to the working levels are dropped).
  const profileLevels = rollId === null ? (profile?.levels ?? null) : null;
  useEffect(() => {
    if (rollId === null) {
      setSaved(null);
      setLevels(profileLevels);
      setShotExposure(null);
      return;
    }
    let alive = true;
    captureLevelsGet(rollId).then(
      (resolved) => {
        if (!alive) return;
        setSaved(resolved);
        setLevels(resolved.levels);
        setShotExposure(null);
      },
      (e: unknown) => {
        if (alive) setError(messageOf(e));
      },
    );
    return () => {
      alive = false;
    };
    // A change of the roll's film stock changes which levels apply.
  }, [rollId, stock, profileLevels]);

  // The light reports every 200 ms while it is connected.
  useEffect(() => {
    let alive = true;
    let stop: (() => void) | undefined;
    onLightStatus((status) => setLight({ connected: true, status })).then(
      (stopListening) => {
        if (alive) stop = stopListening;
        else stopListening();
      },
      () => {
        // No backend (tests, plain browser): there are no readings.
      },
    );
    return () => {
      alive = false;
      stop?.();
    };
  }, []);

  // The light must never be left on when the view closes. A running capture or calibration is cancelled first:
  // otherwise it would light its next colour again after the switch-off.
  useEffect(
    () => () => {
      // Live view first: stops its loop, then takes the camera out of live view.
      liveStop.current?.();
      for (const job of useLibraryStore.getState().jobs) {
        if ((job.kind === 'capture' || job.kind === 'calibration') && job.state === 'running') {
          jobCancel(job.id).catch(() => {
            // The job may have just ended; the switch-off below still runs.
          });
        }
      }
      lightOff().catch(() => {
        // Switching off is always allowed; nothing more can be done here.
      });
    },
    [],
  );

  const connect = async () => {
    setConnecting(true);
    setError(null);
    try {
      await lightConnect(null);
      // Connecting clears a trip; the next reading says how the light is.
      setLight({ connected: true, status: null });
    } catch (e) {
      setError(messageOf(e));
    } finally {
      setConnecting(false);
    }
    await refreshCameras();
  };

  const savedExposure = formatExposure(saved?.exposure ?? null);
  const usable = light.connected && light.status?.tripped !== true && cameras.length > 0;

  return (
    <section aria-labelledby="import-capture" className={styles.section}>
      <h3 id="import-capture" className={styles.heading}>
        {t('import.capture.heading')}
      </h3>
      <p className={styles.muted}>{t('import.capture.intro')}</p>
      <StatusBar
        light={light}
        warnC={profile?.temp_warn_c ?? null}
        cameras={cameras}
        busy={connecting || running}
        error={error}
        onConnect={() => void connect()}
        onRefreshCameras={() => {
          setError(null);
          void refreshCameras();
        }}
      />
      {profile && (
        <div className={styles.captureLive}>
          <h4 className={styles.subheading}>{t('import.capture.liveView.heading')}</h4>
          {profile.camera_backend === 'gphoto' ? (
            cameras.length > 0 && (
              <LiveView
                stopRef={liveStop}
                levels={levels}
                lightLost={!light.connected || light.status?.tripped === true}
                blocked={running || shotBusy || calibrating || capturing}
              />
            )
          ) : (
            <p className={styles.muted}>{t('import.capture.liveView.macOnly')}</p>
          )}
        </div>
      )}
      <CameraSettings cameras={cameras} running={running} onChanging={setSettingsChanging} />
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
      {rolls.length === 0 ? (
        <p role="status">{t('import.files.noRolls')}</p>
      ) : (
        <label className={styles.field}>
          {t('import.files.roll')}
          <select
            value={rollId ?? ''}
            disabled={running}
            onChange={(e) => setChosenRoll(e.target.value)}
          >
            {rolls.map((r) => (
              <option key={r.id} value={r.id}>
                {r.name}
              </option>
            ))}
          </select>
        </label>
      )}
      {levels && (
        <>
          <LevelSliders
            levels={levels}
            onChange={(next) => {
              setLevels(next);
              setShotExposure(null);
            }}
            disabled={running}
          />
          <p role="status">
            {saved && sameLevels(levels, saved.levels)
              ? saved.source === 'stock'
                ? t('import.capture.levels.source.stock', { stock: saved.stock ?? '' })
                : t('import.capture.levels.source.default')
              : t('import.capture.levels.source.changed')}
          </p>
        </>
      )}
      {levels && (
        <TestShot
          disabled={running || !usable || settingsChanging}
          levels={levels}
          onShot={(exposure) => setShotExposure(exposure)}
          onBusyChange={setShotBusy}
        />
      )}
      {levels && (
        <Calibrate
          ready={usable && !settingsChanging}
          busy={running}
          roll={roll}
          levels={levels}
          onBusyChange={setCalibrating}
          onCalibrated={(found, exposure) => {
            setLevels(found);
            setShotExposure(exposure);
          }}
        />
      )}
      {levels && (
        <SaveLevels
          rollId={rollId}
          levels={levels}
          saved={saved}
          stock={stock}
          shot={shotExposure ? { exposure: shotExposure } : null}
          disabled={running}
          onSaved={setSaved}
        />
      )}
      <NextFrameButton
        ready={usable && !settingsChanging && !running}
        roll={roll}
        levels={levels}
        onBusyChange={setCapturing}
      />
    </section>
  );
}
