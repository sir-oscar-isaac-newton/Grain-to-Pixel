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
  Preview,
  Recipe,
  Roll,
  SearchQuery,
  Tag,
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

/**
 * A stand-in for a recipe with the parts the Develop view touches (geometry, base, levels); the
 * rest is not needed by any test. The tag says which one it is.
 */
export function makeRecipe(tag: string, over: Record<string, unknown> = {}): Recipe {
  return {
    v: 1,
    tag,
    geometry: {
      crop: { x: 0.1, y: 0.1, w: 0.8, h: 0.8 },
      angle_deg: 1.5,
      rot90: 1,
      flip_h: true,
      flip_v: false,
    },
    base: { source: 'roll', rgb: [1, 1, 1] },
    invert: { black: [0.1, 0.2, 0.3], white: [0.9, 0.8, 0.7] },
    ...over,
  } as unknown as Recipe;
}

const tagOf = (recipe: Recipe) => (recipe as unknown as { tag?: string }).tag ?? 'none';
/** The colour of every pixel of a preview, by the recipe that made it. */
export const PIXELS: Record<string, [number, number, number]> = {
  saved: [10, 20, 30],
  auto: [200, 100, 50],
};

/** A preview as the backend would answer: tiny, but sized like the real ones (3:2). */
export function makePreview(recipe: Recipe, maxEdge: number, seq: number): Preview {
  const width = Math.round(maxEdge / 16);
  const height = Math.round((maxEdge * 2) / 3 / 16);
  const [r, g, b] = PIXELS[tagOf(recipe)] ?? [128, 128, 128];
  const rgba = new Uint8ClampedArray(width * height * 4);
  for (let i = 0; i < width * height; i += 1) rgba.set([r, g, b, 255], i * 4);
  return { seq, width, height, rgba };
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
  /** Frames whose original cannot be opened in Develop. */
  noScan: new Set<string>(),
  /** Frames whose automatic analysis fails. */
  noAnalysis: new Set<string>(),
  /** The roll's film base, as Develop opens a frame. */
  rollBase: null as [number, number, number] | null,
  /** What picking the base on a frame gives (null: the area is refused). */
  sampledBase: [0.8, 0.5, 0.3] as [number, number, number] | null,
  /** The areas sampled for a film base, in order. */
  samples: [] as Array<{ id: string; region: { x: number; y: number; w: number; h: number } }>,
  /** The areas sampled for a white or black point, in order. */
  levelSamples: [] as Array<{
    id: string;
    recipe: Recipe;
    region: { x: number; y: number; w: number; h: number };
    point: 'white' | 'black';
  }>,
  /** Sampling a white or black point is refused with this message when set. */
  levelsRefused: null as string | null,
  /** The recipes saved, in order. */
  saved: [] as Array<{ id: string; recipe: Recipe }>,
  /** Saving fails with this message when set. */
  saveFails: null as string | null,
  /** The roll bases set, in order. */
  rollBases: [] as Array<{ id: string; base: [number, number, number] | null }>,
  /** Every tag of the library. */
  tags: [] as Tag[],
  /** Tag names put on a frame, by frame id. */
  frameTags: {} as Record<string, string[]>,
  /** Tag names put on a roll, by roll id. */
  rollTags: {} as Record<string, string[]>,
  picked: '/films/Library' as string | null,
};

export const listeners = {
  changed: [] as Array<(roll: string | null, frameIds: string[]) => void>,
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

const keyOf = (name: string) => name.trim().toLowerCase();

function ensureTag(name: string): Tag {
  const found = state.tags.find((t) => keyOf(t.name) === keyOf(name));
  if (found) return found;
  const created = { id: `tag-${state.tags.length + 1}`, name: name.trim() };
  state.tags = [...state.tags, created].sort((a, b) => a.name.localeCompare(b.name));
  return created;
}

function change(names: string[], add: string[], remove: string[]): string[] {
  const kept = names.filter((n) => !remove.some((r) => keyOf(r) === keyOf(n)));
  const added = add.filter((a) => !kept.some((k) => keyOf(k) === keyOf(a)));
  return [...kept, ...added];
}

/** The frames a query finds: a small stand-in for the backend's search (no date matching). */
function matching(q: SearchQuery): FrameSummary[] {
  return state.frames.filter((f) => {
    const roll = state.rolls.find((r) => r.id === f.roll_id);
    const names = [...(state.frameTags[f.id] ?? []), ...(state.rollTags[f.roll_id] ?? [])];
    if (q.roll_id && f.roll_id !== q.roll_id) return false;
    if (q.film_stock && keyOf(roll?.film_stock ?? '') !== keyOf(q.film_stock)) return false;
    if (q.camera && keyOf(roll?.camera ?? '') !== keyOf(q.camera)) return false;
    if (q.iso && roll?.iso_box !== q.iso && roll?.iso_shot !== q.iso) return false;
    for (const id of q.tags_all ?? []) {
      const tag = state.tags.find((t) => t.id === id);
      if (!tag || !names.some((n) => keyOf(n) === keyOf(tag.name))) return false;
    }
    const haystack = [roll?.name, roll?.film_stock, roll?.camera, f.note, ...names]
      .join(' ')
      .toLowerCase();
    return (q.text ?? '')
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean)
      .every((word) => haystack.includes(word));
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
  search: vi.fn(async (q: SearchQuery, page: number, size: number): Promise<FramePage> => {
    const all = matching(q);
    return {
      items: all.slice(page * size, page * size + size),
      total: all.length,
      page,
      page_size: size,
    };
  }),
  tagList: vi.fn(async () => [...state.tags]),
  rollTags: vi.fn(async (id: string) => (state.rollTags[id] ?? []).map(ensureTag)),
  framesOwnTags: vi.fn(async (ids: string[]) => {
    const names = new Set(ids.flatMap((id) => state.frameTags[id] ?? []));
    return state.tags.filter((t) => names.has(t.name));
  }),
  tagApply: vi.fn(async (ids: string[], add: string[], remove: string[]) => {
    add.forEach(ensureTag);
    ids.forEach((id) => (state.frameTags[id] = change(state.frameTags[id] ?? [], add, remove)));
  }),
  tagApplyRoll: vi.fn(async (id: string, add: string[], remove: string[]) => {
    add.forEach(ensureTag);
    state.rollTags[id] = change(state.rollTags[id] ?? [], add, remove);
  }),
  developOpen: vi.fn(async (id: string) => {
    if (state.noScan.has(id)) {
      throw new IpcError({ code: 'NotFound', message: 'This frame has no scan file.' });
    }
    return {
      width: 5616,
      height: 3744,
      has_ir: false,
      recipe: makeRecipe('saved'),
      roll_base: state.rollBase,
    };
  }),
  developAnalyze: vi.fn(async (id: string, base: [number, number, number] | null) => {
    if (state.noAnalysis.has(id)) {
      throw new IpcError({ code: 'InvalidInput', message: 'The scan could not be analysed.' });
    }
    // The levels depend on the base that was given, as the real analysis's do.
    return {
      auto_recipe: makeRecipe('auto', {
        base: { source: 'roll', rgb: base ?? [1, 1, 1] },
        invert: base
          ? { black: [0.01, 0.02, 0.03], white: [0.5, 0.6, 0.7] }
          : { black: [0.05, 0.06, 0.07], white: [0.95, 0.85, 0.75] },
      }),
    };
  }),
  developSampleBase: vi.fn(
    async (id: string, region: { x: number; y: number; w: number; h: number }) => {
      state.samples.push({ id, region });
      if (!state.sampledBase) {
        throw new IpcError({
          code: 'InvalidInput',
          message: 'That area is too dark to be film base.',
        });
      }
      return state.sampledBase;
    },
  ),
  developSampleLevels: vi.fn(
    async (
      id: string,
      recipe: Recipe,
      region: { x: number; y: number; w: number; h: number },
      point: 'white' | 'black',
    ) => {
      state.levelSamples.push({ id, recipe, region, point });
      if (state.levelsRefused) {
        throw new IpcError({ code: 'InvalidInput', message: state.levelsRefused });
      }
      const current = (recipe as unknown as { invert: { black: number[]; white: number[] } })
        .invert;
      return point === 'white'
        ? { black: current.black, white: [0.6, 0.7, 0.8] }
        : { black: [0.02, 0.03, 0.04], white: current.white };
    },
  ),
  frameGet: vi.fn(async (id: string) => {
    const found = state.frames.find((f) => f.id === id);
    if (!found) throw new IpcError({ code: 'NotFound', message: 'No such frame.' });
    return found;
  }),
  recipeSave: vi.fn(async (id: string, recipe: Recipe) => {
    if (state.saveFails) throw new IpcError({ code: 'Io', message: state.saveFails });
    state.saved.push({ id, recipe });
    return recipe;
  }),
  rollSetFilmBase: vi.fn(async (id: string, base: [number, number, number] | null) => {
    state.rollBases.push({ id, base });
    state.rollBase = base;
    state.rolls = state.rolls.map((r) => (r.id === id ? { ...r, film_base_rgb: base } : r));
    return state.rolls.find((r) => r.id === id) as Roll;
  }),
  logClientEvent: vi.fn(async () => undefined),
  developRender: vi.fn(async (_id: string, recipe: Recipe, maxEdge: number, seq: number) =>
    makePreview(recipe, maxEdge, seq),
  ),
  frameThumbnail: vi.fn(async (id: string) => {
    if (state.noThumb.has(id)) throw new IpcError({ code: 'NotFound', message: 'no thumbnail' });
    return new Uint8Array([0xff, 0xd8, 1, 2, 3]).buffer;
  }),
  importFiles: vi.fn(async () => 'job-1'),
  jobCancel: vi.fn(async () => true),
  onLibraryChanged: vi.fn((cb: (roll: string | null, frameIds: string[]) => void) =>
    subscribe(listeners.changed, cb),
  ),
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
  state.noScan = new Set();
  state.noAnalysis = new Set();
  state.sampledBase = [0.8, 0.5, 0.3];
  state.rollBase = null;
  state.saved = [];
  state.samples = [];
  state.levelSamples = [];
  state.levelsRefused = null;
  state.saveFails = null;
  state.rollBases = [];
  state.tags = [];
  state.frameTags = {};
  state.rollTags = {};
  state.picked = '/films/Library';
  listeners.changed.length = 0;
  listeners.progress.length = 0;
  listeners.done.length = 0;
  listeners.error.length = 0;
  Object.values(ipc).forEach((fn) => {
    if (typeof fn === 'function' && 'mockClear' in fn) {
      // Clearing the calls keeps the implementations, but a result queued with Once and never
      // used would leak into the next test, so those are dropped too.
      (fn as ReturnType<typeof vi.fn>).mockClear();
      (fn as ReturnType<typeof vi.fn>).mockReset();
    }
  });
}
