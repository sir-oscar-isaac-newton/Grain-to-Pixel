import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MutableRefObject,
  type ReactNode,
} from 'react';
import {
  cameraList,
  cameraSettingsGet,
  captureLevelsGet,
  captureProfileGet,
  jobCancel,
  lightConnect,
  lightOff,
  onLightStatus,
  type CameraInfo,
  type CameraSetting,
  type CaptureLevels,
  type CaptureProfile,
  type ExposureInfo,
  type Levels,
  type Roll,
} from '../../../shared/ipc';
import { messageOf, useLibraryStore } from '../../library/library-store';
import type { Area } from './area';
import { valuesOf, type CameraValues } from './exposure';
import type { LightState } from './StatusBar';

export const sameLevels = (a: Levels, b: Levels) => a.every((v, i) => v === b[i]);

/** Whether a job is one that shoots with the camera and the light. */
const isShooting = (kind: string) =>
  kind === 'capture' || kind === 'calibration' || kind === 'flat_field';

/** What the levels of the roll were calibrated with in this session. */
export interface Calibrated {
  levels: Levels;
  exposure: ExposureInfo;
}

/**
 * The rig the camera steps share: the light, the cameras, the camera's settings and the levels, so
 * that moving between steps keeps the connection and the levels.
 */
export interface CaptureRig {
  rollId: string;
  roll: Roll | null;
  /** The roll's film stock, trimmed; null when it has none. */
  stock: string | null;
  /** True while a capture, calibration or flat field job runs. */
  running: boolean;
  light: LightState;
  cameras: CameraInfo[];
  profile: CaptureProfile | null;
  connecting: boolean;
  error: string | null;
  clearError: () => void;
  /** The light answers, is not tripped, and there is a camera. */
  usable: boolean;
  saved: CaptureLevels | null;
  setSaved: (saved: CaptureLevels) => void;
  levels: Levels | null;
  setLevels: (levels: Levels) => void;
  shotExposure: ExposureInfo | null;
  setShotExposure: (exposure: ExposureInfo | null) => void;
  /** The camera's ISO, shutter and aperture as last read; null until they are. */
  camera: CameraValues | null;
  onCameraSettings: (settings: CameraSetting[]) => void;
  settingsChanging: boolean;
  setSettingsChanging: (changing: boolean) => void;
  shotBusy: boolean;
  setShotBusy: (busy: boolean) => void;
  calibrating: boolean;
  setCalibrating: (busy: boolean) => void;
  capturing: boolean;
  setCapturing: (busy: boolean) => void;
  focusing: boolean;
  setFocusing: (busy: boolean) => void;
  /** Something is using the camera: the guide's navigation holds still. */
  busy: boolean;
  calibrationArea: Area | null;
  setCalibrationArea: (area: Area | null) => void;
  liveStop: MutableRefObject<(() => void) | null>;
  calibrateStart: MutableRefObject<(() => void) | null>;
  /** True once the levels were changed by hand or calibrated. */
  levelsHandled: boolean;
  setLevelsHandled: (handled: boolean) => void;
  calibrated: Calibrated | null;
  setCalibrated: (calibrated: Calibrated | null) => void;
  /** Session only: the user chose the global levels for this roll. */
  globalLevels: boolean;
  setGlobalLevels: (chosen: boolean) => void;
  /** Session only: the user skipped the flat field of this roll. */
  flatFieldSkipped: boolean;
  setFlatFieldSkipped: (skipped: boolean) => void;
  refreshCameras: () => Promise<void>;
  connect: () => Promise<void>;
  /** Called when a step is left: stops live view and switches the light off. */
  leaveStep: () => void;
}

const RigContext = createContext<CaptureRig | null>(null);

/** The rig of the camera steps, or null outside a provider or while it is idle. */
export function useOptionalCaptureRig(): CaptureRig | null {
  return useContext(RigContext);
}

/** The rig of the camera steps; the steps are always shown inside a provider. */
export function useCaptureRig(): CaptureRig {
  const rig = useContext(RigContext);
  if (!rig) throw new Error('useCaptureRig needs a CaptureRigProvider with a roll');
  return rig;
}

/**
 * Holds the rig while `rollId` is set. With null the rig is idle: nothing is asked of the light or
 * the camera. When it goes idle or unmounts, live view stops, running jobs are cancelled and the
 * light goes off.
 */
export function CaptureRigProvider({
  rollId,
  children,
}: {
  rollId: string | null;
  children: ReactNode;
}) {
  const active = rollId !== null;
  const running = useLibraryStore((s) =>
    s.jobs.some((j) => isShooting(j.kind) && j.state === 'running'),
  );
  const roll = useLibraryStore((s) => s.rolls.find((r) => r.id === rollId) ?? null);
  const stock = roll?.film_stock?.trim() || null;

  const [light, setLight] = useState<LightState>({ connected: false, status: null });
  const [cameras, setCameras] = useState<CameraInfo[]>([]);
  const [profile, setProfile] = useState<CaptureProfile | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<CaptureLevels | null>(null);
  const [levels, setLevels] = useState<Levels | null>(null);
  const [shotExposure, setShotExposure] = useState<ExposureInfo | null>(null);
  const [camera, setCamera] = useState<CameraValues | null>(null);
  const [settingsChanging, setSettingsChanging] = useState(false);
  const [shotBusy, setShotBusy] = useState(false);
  const [calibrating, setCalibrating] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const [focusing, setFocusing] = useState(false);
  const [calibrationArea, setCalibrationArea] = useState<Area | null>(null);
  const [levelsHandled, setLevelsHandled] = useState(false);
  const [calibrated, setCalibrated] = useState<Calibrated | null>(null);
  const [globalLevels, setGlobalLevels] = useState(false);
  const [flatFieldSkipped, setFlatFieldSkipped] = useState(false);
  const liveStop = useRef<(() => void) | null>(null);
  const calibrateStart = useRef<(() => void) | null>(null);

  const refreshCameras = useCallback(async () => {
    try {
      setCameras(await cameraList());
    } catch (e) {
      setCameras([]);
      setError(messageOf(e));
    }
  }, []);

  useEffect(() => {
    if (!active) return;
    void refreshCameras();
    captureProfileGet().then(setProfile, (e: unknown) => setError(messageOf(e)));
  }, [active, refreshCameras]);

  // The camera's settings are read as soon as there is a camera, so a guide resumed past Set up
  // still knows them; without a camera there are none to know.
  const hasCamera = cameras.length > 0;
  useEffect(() => {
    if (!active || !hasCamera) {
      setCamera(null);
      return;
    }
    let alive = true;
    cameraSettingsGet().then(
      (settings) => {
        if (alive) setCamera(valuesOf(settings));
      },
      () => {
        // Unreadable settings stay unknown; Set up shows the error when it reads them.
      },
    );
    return () => {
      alive = false;
    };
  }, [active, hasCamera]);

  // Another roll starts afresh: the session's choices are dropped.
  useEffect(() => {
    setGlobalLevels(false);
    setFlatFieldSkipped(false);
  }, [rollId]);

  // The roll's levels: saved for its film stock, else the default. Unsaved changes to the working
  // levels are dropped when they are read again.
  useEffect(() => {
    if (rollId === null) {
      setSaved(null);
      setLevels(null);
      return;
    }
    let alive = true;
    captureLevelsGet(rollId).then(
      (resolved) => {
        if (!alive) return;
        setSaved(resolved);
        setLevels(resolved.levels);
        setShotExposure(null);
        setLevelsHandled(false);
        setCalibrated(null);
      },
      (e: unknown) => {
        if (alive) setError(messageOf(e));
      },
    );
    return () => {
      alive = false;
    };
    // A change of the roll's film stock changes which levels apply.
  }, [rollId, stock]);

  // The light reports every 200 ms while it is connected.
  useEffect(() => {
    if (!active) return;
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
  }, [active]);

  // The light must never be left on when the rig goes: a running capture or calibration is
  // cancelled first, otherwise it would light its next colour again after the switch-off.
  useEffect(() => {
    if (!active) return;
    return () => {
      // Live view first: stops its loop, then takes the camera out of live view.
      liveStop.current?.();
      for (const job of useLibraryStore.getState().jobs) {
        if (isShooting(job.kind) && job.state === 'running') {
          jobCancel(job.id).catch(() => {
            // The job may have just ended; the switch-off below still runs.
          });
        }
      }
      lightOff().catch(() => {
        // Switching off is always allowed; nothing more can be done here.
      });
    };
  }, [active]);

  const connect = useCallback(async () => {
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
  }, [refreshCameras]);

  const leaveStep = useCallback(() => {
    liveStop.current?.();
    liveStop.current = null;
    lightOff().catch(() => {
      // Switching off is always allowed; nothing more can be done here.
    });
  }, []);

  const onCameraSettings = useCallback(
    (settings: CameraSetting[]) => setCamera(valuesOf(settings)),
    [],
  );
  const clearError = useCallback(() => setError(null), []);

  const usable = light.connected && light.status?.tripped !== true && cameras.length > 0;
  const busy = running || shotBusy || calibrating || capturing || focusing;

  const rig = useMemo<CaptureRig | null>(
    () =>
      rollId === null
        ? null
        : {
            rollId,
            roll,
            stock,
            running,
            light,
            cameras,
            profile,
            connecting,
            error,
            clearError,
            usable,
            saved,
            setSaved,
            levels,
            setLevels,
            shotExposure,
            setShotExposure,
            camera,
            onCameraSettings,
            settingsChanging,
            setSettingsChanging,
            shotBusy,
            setShotBusy,
            calibrating,
            setCalibrating,
            capturing,
            setCapturing,
            focusing,
            setFocusing,
            busy,
            calibrationArea,
            setCalibrationArea,
            liveStop,
            calibrateStart,
            levelsHandled,
            setLevelsHandled,
            calibrated,
            setCalibrated,
            globalLevels,
            setGlobalLevels,
            flatFieldSkipped,
            setFlatFieldSkipped,
            refreshCameras,
            connect,
            leaveStep,
          },
    [
      rollId,
      roll,
      stock,
      running,
      light,
      cameras,
      profile,
      connecting,
      error,
      clearError,
      usable,
      saved,
      levels,
      shotExposure,
      camera,
      onCameraSettings,
      settingsChanging,
      shotBusy,
      calibrating,
      capturing,
      focusing,
      busy,
      calibrationArea,
      levelsHandled,
      calibrated,
      globalLevels,
      flatFieldSkipped,
      refreshCameras,
      connect,
      leaveStep,
    ],
  );

  return <RigContext.Provider value={rig}>{children}</RigContext.Provider>;
}
