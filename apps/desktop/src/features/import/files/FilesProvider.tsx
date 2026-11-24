import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type ReactNode,
  type SetStateAction,
} from 'react';
import { useTranslation } from 'react-i18next';
import {
  importFiles,
  pickCameraFiles,
  pickTiffFiles,
  tripletGroup,
  type Roll,
} from '../../../shared/ipc';
import { messageOf, useLibraryStore } from '../../library/library-store';
import { importPaths, isComplete, toGroups, type Group } from '../triplets';

/** What the files are: camera files (red, green and blue shots to make triplets) or TIFF scans. */
export type FilesKind = 'camera' | 'tiff';

/**
 * What the files steps share: the kind of file, the files chosen and what came of starting the
 * import, so that moving between steps keeps them.
 */
export interface Files {
  rollId: string;
  roll: Roll | null;
  readOnly: boolean;
  kind: FilesKind;
  setKind: (kind: FilesKind) => void;
  /** The camera files, grouped into triplets. */
  groups: Group[];
  setGroups: Dispatch<SetStateAction<Group[]>>;
  /** The TIFF scans chosen: one frame each. */
  tiffPaths: string[];
  removeTiff: (path: string) => void;
  /** The paths the import starts with, for the kind chosen. */
  paths: string[];
  /** How many triplets (camera files) or scans (TIFF) will be imported. */
  count: number;
  /** Camera files only: the groups that are not a whole triplet. */
  attention: number;
  /** A file picker or the grouping is under way, or the import is being started. */
  busy: boolean;
  /** Why the files could not be chosen or grouped. */
  chooseError: string | null;
  /** Why the import could not be started. */
  startError: string | null;
  /** The import was started and the files are cleared. */
  started: boolean;
  choose: () => Promise<void>;
  start: () => Promise<void>;
}

const FilesContext = createContext<Files | null>(null);

/** The files state of the files steps, or null outside a provider or while it is idle. */
export function useOptionalFiles(): Files | null {
  return useContext(FilesContext);
}

/** The files state of the files steps; the steps are always shown inside a provider. */
export function useFiles(): Files {
  const files = useContext(FilesContext);
  if (!files) throw new Error('useFiles needs a FilesProvider with a roll');
  return files;
}

/**
 * Holds the files state while `rollId` is set. With null it is idle. Another roll, or leaving the
 * files source, drops the files chosen.
 */
export function FilesProvider({
  rollId,
  children,
}: {
  rollId: string | null;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const roll = useLibraryStore((s) => s.rolls.find((r) => r.id === rollId) ?? null);
  const readOnly = useLibraryStore((s) => s.library?.read_only ?? false);
  const addJob = useLibraryStore((s) => s.addJob);

  const [kind, setKindState] = useState<FilesKind>('camera');
  const [groups, setGroups] = useState<Group[]>([]);
  const [tiffPaths, setTiffPaths] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [chooseError, setChooseError] = useState<string | null>(null);
  const [startError, setStartError] = useState<string | null>(null);
  const [started, setStarted] = useState(false);
  // Counts the times the state was dropped, so an answer that comes late is ignored.
  const epoch = useRef(0);

  useEffect(() => {
    epoch.current += 1;
    setKindState('camera');
    setGroups([]);
    setTiffPaths([]);
    setBusy(false);
    setChooseError(null);
    setStartError(null);
    setStarted(false);
  }, [rollId]);

  const paths = kind === 'camera' ? importPaths(groups) : tiffPaths;
  const count =
    kind === 'camera'
      ? groups.filter((g) => g.confirmed && isComplete(g)).length
      : tiffPaths.length;
  const attention = groups.filter((g) => !isComplete(g)).length;

  const setKind = (next: FilesKind) => {
    setKindState(next);
    setChooseError(null);
    setStartError(null);
    setStarted(false);
  };

  const choose = async () => {
    const mine = epoch.current;
    setBusy(true);
    setChooseError(null);
    setStarted(false);
    try {
      if (kind === 'camera') {
        const picked = await pickCameraFiles();
        if (picked.length === 0) return;
        const found = toGroups(await tripletGroup(picked));
        if (epoch.current === mine) setGroups(found);
      } else {
        const picked = await pickTiffFiles();
        if (picked.length > 0 && epoch.current === mine) setTiffPaths(picked);
      }
    } catch (e) {
      if (epoch.current === mine) {
        setChooseError(
          t(kind === 'camera' ? 'import.files.readFailed' : 'import.files.tiffFailed', {
            message: messageOf(e),
          }),
        );
      }
    } finally {
      if (epoch.current === mine) setBusy(false);
    }
  };

  const start = async () => {
    if (!roll) return;
    const mine = epoch.current;
    setBusy(true);
    setStartError(null);
    try {
      const id = await importFiles(paths, roll.id, kind === 'camera' ? 'RgbTriplet' : 'TiffScan');
      addJob({ id, kind: 'import', rollName: roll.name, fraction: 0, state: 'running' });
      if (epoch.current === mine) {
        if (kind === 'camera') setGroups([]);
        else setTiffPaths([]);
        setStarted(true);
      }
    } catch (e) {
      if (epoch.current === mine) {
        setStartError(t('import.files.startFailed', { message: messageOf(e) }));
      }
    } finally {
      if (epoch.current === mine) setBusy(false);
    }
  };

  const files: Files | null =
    rollId === null
      ? null
      : {
          rollId,
          roll,
          readOnly,
          kind,
          setKind,
          groups,
          setGroups,
          tiffPaths,
          removeTiff: (path) => setTiffPaths((all) => all.filter((p) => p !== path)),
          paths,
          count,
          attention,
          busy,
          chooseError,
          startError,
          started,
          choose,
          start,
        };

  return <FilesContext.Provider value={files}>{children}</FilesContext.Provider>;
}
