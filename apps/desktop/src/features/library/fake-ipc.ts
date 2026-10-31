// An in-memory stand-in for `shared/ipc`, for the Library view tests:
//   vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);
import { vi } from 'vitest';
import type {
  AppError,
  FolderCheck,
  FramePage,
  FrameSummary,
  ImportResult,
  LibraryDetails,
  Roll,
} from '../../shared/ipc';

export class IpcError extends Error {
  readonly error: AppError;
  constructor(error: AppError) {
    super(error.message);
    this.name = 'IpcError';
    this.error = error;
  }
}

export function libraryDetails(over: Partial<LibraryDetails> = {}): LibraryDetails {
  return {
    path: '/films/Library',
    name: 'Film',
    created_at: '2026-10-30T10:00:00Z',
    read_only: false,
    schema_version: 4,
    free_bytes: 54_900_000_000,
    last_verify_at: null,
    last_verify_problems: null,
    ...over,
  };
}

let rollCounter = 0;
export function makeRoll(over: Partial<Roll> = {}): Roll {
  rollCounter += 1;
  return {
    id: `roll-${rollCounter}`,
    name: `Roll ${rollCounter}`,
    film_stock: null,
    format: '35mm',
    camera: null,
    iso_box: null,
    iso_shot: null,
    shot_from: null,
    shot_to: null,
    notes: '',
    film_base_rgb: null,
    film_base_confidence: null,
    created_at: '2026-10-30T10:00:00Z',
    ...over,
  };
}

/** `count` frames of a roll, numbered from 1. */
export function makeFrames(rollId: string, count: number, prefix = rollId): FrameSummary[] {
  return Array.from({ length: count }, (_, i) => ({
    id: `${prefix}-f${String(i + 1).padStart(5, '0')}`,
    roll_id: rollId,
    index_in_roll: i + 1,
    kind: 'ScanRgbIr',
    width: 5616,
    height: 3744,
    has_ir: true,
    note: '',
    edited_at: null,
    has_thumb: true,
  }));
}

export const state = {
  details: null as LibraryDetails | null,
  lastLibrary: null as string | null,
  rolls: [] as Roll[],
  frames: [] as FrameSummary[],
  folder: { exists: true, is_empty: true, has_library: false } as Pick<
    FolderCheck,
    'exists' | 'is_empty' | 'has_library'
  >,
  noThumb: new Set<string>(),
  picked: '/films/Library' as string | null,
};

export const listeners = {
  changed: [] as Array<(roll: string | null) => void>,
  progress: [] as Array<(e: { id: string; stage: string; fraction: number }) => void>,
  done: [] as Array<
    (e: {
      id: string;
      result: { kind: 'Finished' } | ({ kind: 'Imported' } & ImportResult);
    }) => void
  >,
  error: [] as Array<(e: { id: string; error: AppError }) => void>,
};

function subscribe<T>(list: T[], cb: T) {
  list.push(cb);
  return Promise.resolve(() => {
    const at = list.indexOf(cb);
    if (at >= 0) list.splice(at, 1);
  });
}

function pageOf(rollId: string | null | undefined, page: number, size: number): FramePage {
  const all = state.frames.filter((f) => !rollId || f.roll_id === rollId);
  return {
    items: all.slice(page * size, page * size + size),
    total: all.length,
    page,
    page_size: size,
  };
}

export const ipc = {
  IpcError,
  libraryInfo: vi.fn(async () => ({ app_version: '1.0.0', library: state.details })),
  libraryCreate: vi.fn(async (path: string, name: string) => {
    state.details = libraryDetails({ path, name });
    return { app_version: '1.0.0', library: state.details };
  }),
  libraryOpen: vi.fn(
    async (path: string): Promise<{ app_version: string; library: LibraryDetails | null }> => {
      state.details = state.details ?? libraryDetails({ path });
      return { app_version: '1.0.0', library: state.details };
    },
  ),
  settingsGet: vi.fn(async () => ({ last_library: state.lastLibrary })),
  folderCheck: vi.fn(async (path: string): Promise<FolderCheck> => ({
    path,
    free_bytes: 54_900_000_000,
    ...state.folder,
  })),
  pickFolder: vi.fn(async () => state.picked),
  pickTiffFiles: vi.fn(async (): Promise<string[]> => []),
  rollList: vi.fn(async () => [...state.rolls]),
  rollCreate: vi.fn(async (input: Partial<Roll>) => {
    const created = makeRoll(input);
    state.rolls = [created, ...state.rolls];
    return created;
  }),
  rollUpdate: vi.fn(async (id: string, input: Partial<Roll>) => {
    state.rolls = state.rolls.map((r) => (r.id === id ? { ...r, ...input } : r));
    return state.rolls.find((r) => r.id === id) as Roll;
  }),
  rollDelete: vi.fn(async (id: string) => {
    const frames = state.frames.filter((f) => f.roll_id === id).length;
    state.rolls = state.rolls.filter((r) => r.id !== id);
    state.frames = state.frames.filter((f) => f.roll_id !== id);
    return { frames_deleted: frames, files_left: [] as string[] };
  }),
  frameList: vi.fn(async (q: { roll_id?: string | null; page: number; page_size: number }) =>
    pageOf(q.roll_id, q.page, q.page_size),
  ),
  frameThumbnail: vi.fn(async (id: string) => {
    if (state.noThumb.has(id)) throw new IpcError({ code: 'NotFound', message: 'no thumbnail' });
    return new Uint8Array([0xff, 0xd8, 1, 2, 3]).buffer;
  }),
  importFiles: vi.fn(async () => 'job-1'),
  jobCancel: vi.fn(async () => true),
  onLibraryChanged: vi.fn((cb: (roll: string | null) => void) => subscribe(listeners.changed, cb)),
  onJobProgress: vi.fn((cb: (typeof listeners.progress)[number]) =>
    subscribe(listeners.progress, cb),
  ),
  onJobDone: vi.fn((cb: (typeof listeners.done)[number]) => subscribe(listeners.done, cb)),
  onJobError: vi.fn((cb: (typeof listeners.error)[number]) => subscribe(listeners.error, cb)),
};

/** Forgets everything and clears the call history; call before each test. */
export function resetFake() {
  state.details = null;
  state.lastLibrary = null;
  state.rolls = [];
  state.frames = [];
  state.folder = { exists: true, is_empty: true, has_library: false };
  state.noThumb = new Set();
  state.picked = '/films/Library';
  listeners.changed.length = 0;
  listeners.progress.length = 0;
  listeners.done.length = 0;
  listeners.error.length = 0;
  Object.values(ipc).forEach((fn) => {
    if (typeof fn === 'function' && 'mockClear' in fn) (fn as ReturnType<typeof vi.fn>).mockClear();
  });
}
