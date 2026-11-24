import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { useTranslation } from 'react-i18next';
import {
  scanPreview,
  scanPreviewCancel,
  scanResumePoint,
  scanStart,
  scannerList,
  type DeviceInfo,
  type Roll,
  type RollPreviewImage,
} from '../../../shared/ipc';
import { messageOf, useLibraryStore } from '../../library/library-store';
import { frameProblems, MAX_FRAMES } from './frames';

/**
 * What the scanner steps share: the scanner, the frames to scan, the settings and the roll preview,
 * so that moving between steps keeps them.
 */
export interface Scan {
  rollId: string;
  roll: Roll | null;
  /** True while a scan job runs. */
  scanning: boolean;
  /** The inputs are off: a scan runs, or the library is read-only. */
  locked: boolean;
  /** The scanners found; null until they were looked for. */
  devices: DeviceInfo[] | null;
  deviceError: string | null;
  /** The scanner the scan uses. */
  device: DeviceInfo | null;
  /** Looks for the scanner, unless that was done already in this guide. */
  ensureDevices: () => void;
  refreshDevices: () => void;
  first: string;
  setFirst: (text: string) => void;
  last: string;
  setLast: (text: string) => void;
  /** Sets the first frame, as the user's own choice. */
  startFrom: (n: number) => void;
  problems: { first?: string; last?: string };
  /** The frames typed are a range the scanner can be asked for. */
  valid: boolean;
  /** The frame where this roll's scanning continues (41 when every frame is scanned). */
  resume: number;
  /** Every frame of the roll has been scanned. */
  complete: boolean;
  dpi: number;
  setDpi: (dpi: number) => void;
  samples: number;
  setSamples: (samples: number) => void;
  /** Whether the infrared is scanned too. */
  ir: boolean;
  setIr: (ir: boolean) => void;
  previewing: boolean;
  preview: RollPreviewImage | null;
  previewError: string | null;
  previewRoll: () => Promise<void>;
  cancelPreview: () => void;
  startError: string | null;
  /** True from the moment Start is pressed until the scan's job is known or the start failed. */
  starting: boolean;
  start: () => Promise<void>;
}

const ScanContext = createContext<Scan | null>(null);

/** The scan state of the scanner steps, or null outside a provider or while it is idle. */
export function useOptionalScan(): Scan | null {
  return useContext(ScanContext);
}

/** The scan state of the scanner steps; the steps are always shown inside a provider. */
export function useScan(): Scan {
  const scan = useContext(ScanContext);
  if (!scan) throw new Error('useScan needs a ScanProvider with a roll');
  return scan;
}

/** Whether the roll's film is scanned with the infrared to start with: not black-and-white silver film. */
const infraredFor = (roll: Roll | null) => roll?.film_type !== 'bw_silver';

/**
 * Holds the scan state while `rollId` is set. With null it is idle: nothing is asked of the
 * scanner. Another roll starts afresh, and when the guide leaves the scanner (another source, another
 * view) the state is dropped and a running roll preview is stopped.
 */
export function ScanProvider({ rollId, children }: { rollId: string | null; children: ReactNode }) {
  const { t } = useTranslation();
  const active = rollId !== null;
  const roll = useLibraryStore((s) => s.rolls.find((r) => r.id === rollId) ?? null);
  const readOnly = useLibraryStore((s) => s.library?.read_only ?? false);
  const scanning = useLibraryStore((s) =>
    s.jobs.some((j) => j.kind === 'scan' && j.state === 'running'),
  );
  const addJob = useLibraryStore((s) => s.addJob);

  const [devices, setDevices] = useState<DeviceInfo[] | null>(null);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  const [first, setFirstText] = useState('1');
  const [firstEdited, setFirstEdited] = useState(false);
  const [last, setLast] = useState(String(MAX_FRAMES));
  const [dpi, setDpi] = useState<number>(4000);
  const [samples, setSamples] = useState<number>(1);
  // Null until the user chooses: the roll's film type decides until then.
  const [irChoice, setIrChoice] = useState<boolean | null>(null);
  const [resume, setResume] = useState(1);
  const [startError, setStartError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  // Set at once (state is only seen on the next render), so a second click is ignored.
  const startingRef = useRef(false);
  const [previewing, setPreviewing] = useState(false);
  const [preview, setPreview] = useState<RollPreviewImage | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const looked = useRef(false);
  // These count how often the scanner state, and the roll's state, were dropped, so that an
  // answer which comes late is ignored.
  const lookups = useRef(0);
  const rolls = useRef(0);
  const previewingRef = useRef(false);

  const device = devices?.[0] ?? null;
  const problems = frameProblems(first, last);
  const valid = !problems.first && !problems.last;
  const complete = resume > MAX_FRAMES;
  const ir = irChoice ?? infraredFor(roll);

  const setPreviewingNow = (on: boolean) => {
    previewingRef.current = on;
    setPreviewing(on);
  };

  // Another roll, or none, starts afresh. This runs before the effect below that asks for the
  // roll's resume point.
  useEffect(() => {
    rolls.current += 1;
    setFirstText('1');
    setFirstEdited(false);
    setLast(String(MAX_FRAMES));
    setDpi(4000);
    setSamples(1);
    setIrChoice(null);
    setResume(1);
    setStartError(null);
    setPreview(null);
    setPreviewError(null);
    previewingRef.current = false;
    setPreviewing(false);
  }, [rollId]);

  // The scanner is looked for again after the guide has been away from it.
  useEffect(() => {
    if (active) return;
    lookups.current += 1;
    looked.current = false;
    setDevices(null);
    setDeviceError(null);
  }, [active]);

  const refreshDevices = useCallback(() => {
    looked.current = true;
    const mine = lookups.current;
    setDeviceError(null);
    scannerList().then(
      (list) => {
        if (lookups.current === mine) setDevices(list);
      },
      (e: unknown) => {
        if (lookups.current !== mine) return;
        setDevices([]);
        setDeviceError(messageOf(e));
      },
    );
  }, []);

  const ensureDevices = useCallback(() => {
    if (!looked.current) refreshDevices();
  }, [refreshDevices]);

  // Asks where this roll's scanning should continue, now and whenever a scan ends.
  useEffect(() => {
    if (rollId === null || scanning) return;
    let alive = true;
    scanResumePoint(rollId).then(
      (n) => {
        if (!alive) return;
        setResume(n);
        if (!firstEdited) setFirstText(String(Math.min(n, MAX_FRAMES)));
      },
      () => {
        // The roll was just removed; the guide follows the store.
      },
    );
    return () => {
      alive = false;
    };
  }, [rollId, scanning, firstEdited]);

  // A running roll preview is stopped when the guide leaves the scanner or the roll.
  useEffect(() => {
    if (!active) return;
    return () => {
      if (!previewingRef.current) return;
      scanPreviewCancel().catch(() => {
        // The preview may have just ended.
      });
    };
  }, [active, rollId]);

  const setFirst = (text: string) => {
    setFirstEdited(true);
    setFirstText(text);
  };

  const start = async () => {
    if (!roll || !device || !valid || startingRef.current || previewingRef.current) return;
    const mine = rolls.current;
    startingRef.current = true;
    setStarting(true);
    setStartError(null);
    try {
      const job = await scanStart(roll.id, {
        device_id: device.id,
        first_frame: Number(first),
        last_frame: Number(last),
        dpi,
        samples,
        ir,
        autofocus: false,
      });
      addJob({ id: job, kind: 'scan', rollName: roll.name, fraction: 0, state: 'running' });
    } catch (e) {
      if (rolls.current === mine) {
        setStartError(t('import.scanner.startFailed', { message: messageOf(e) }));
      }
    } finally {
      startingRef.current = false;
      setStarting(false);
    }
  };

  const previewRoll = async () => {
    if (!device) return;
    const mine = rolls.current;
    setPreviewingNow(true);
    setPreview(null);
    setPreviewError(null);
    try {
      const image = await scanPreview(device.id);
      if (rolls.current === mine) setPreview(image);
    } catch (e) {
      // A cancel is the user's own doing, not a failure.
      const cancelled = (e as { error?: { code?: string } }).error?.code === 'Cancelled';
      if (!cancelled && rolls.current === mine) {
        setPreviewError(t('import.scanner.previewFailed', { message: messageOf(e) }));
      }
    } finally {
      if (rolls.current === mine) setPreviewingNow(false);
    }
  };

  const cancelPreview = () => {
    scanPreviewCancel().catch(() => {
      // The preview may have just ended.
    });
  };

  const scan: Scan | null =
    rollId === null
      ? null
      : {
          rollId,
          roll,
          scanning,
          locked: scanning || readOnly,
          devices,
          deviceError,
          device,
          ensureDevices,
          refreshDevices,
          first,
          setFirst,
          last,
          setLast,
          startFrom: (n) => setFirst(String(n)),
          problems,
          valid,
          resume,
          complete,
          dpi,
          setDpi,
          samples,
          setSamples,
          ir,
          setIr: setIrChoice,
          previewing,
          preview,
          previewError,
          previewRoll,
          cancelPreview,
          startError,
          starting,
          start,
        };

  return <ScanContext.Provider value={scan}>{children}</ScanContext.Provider>;
}
