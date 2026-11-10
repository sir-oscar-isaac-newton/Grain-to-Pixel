// An in-memory stand-in for `shared/ipc`, for the Library view tests:
//   vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);
import { vi } from 'vitest';
import type {
  AppError,
  CameraInfo,
  CameraSetting,
  CameraSettingKind,
  CaptureLevels,
  CaptureProfile,
  CaptureStep,
  Counters,
  DeviceInfo,
  FolderCheck,
  FramePage,
  FrameSummary,
  ImportResult,
  LevelsScope,
  LibraryDetails,
  LightInfo,
  LightStatus,
  Look,
  Preview,
  Recipe,
  Roll,
  RollPreviewImage,
  ScanRequest,
  SearchQuery,
  Tag,
  TestShot,
  TripletCandidate,
  UpdateOutcome,
  VerifyResult,
  FocusCheck,
  ZoomStep,
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
    frame_size: null,
    has_flat_field: false,
    color_balance: null,
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

/** The master curve the fake gives for the Film log look (the shape matters, not the values). */
export const FILM_LOG_CURVE: Array<[number, number]> = [
  [0, 0.03],
  [0.5, 0.2],
  [1, 0.31],
];

/** The master curve the fake gives for the Frontier look. */
export const FRONTIER_CURVE: Array<[number, number]> = [
  [0, 0],
  [0.5, 0.18],
  [1, 1],
];

/** What the fake accepts of a settings patch. */
interface SettingsPatchShape {
  starting_look?: 'neutral' | 'film_log' | 'frontier';
  flags?: Partial<typeof DEFAULT_FLAGS>;
  keys?: Record<string, string>;
  focus_reference?: FocusCheck['raw_map'];
  clear_focus_reference?: boolean;
}

/** The answer of the focus check before a test changes it. */
export const DEFAULT_FOCUS_CHECK = {
  crop: 'AAAA',
  crop_width: 512,
  crop_height: 512,
  sharpness: 12.34,
  map: [
    [0.8, 0.9, 0.8],
    [0.9, 1, 0.9],
    [0.8, 0.9, 0.8],
  ],
  raw_map: [
    [0.7, 0.85, 0.7],
    [0.85, 1, 0.85],
    [0.7, 0.85, 0.7],
  ],
  pattern: { kind: 'bowed' },
  exposure: { exposure_time: { num: 1, den: 60 }, iso: 200, f_number: { num: 56, den: 10 } },
  against_reference: false,
} as FocusCheck;

/** The feature flags before anyone changes them (spec section 10). */
export const DEFAULT_FLAGS = {
  dust_removal: true,
  triplet_import: true,
  guided_capture: true,
  dev_overlay: false,
  update_check: true,
};

/** The camera of the fake: ISO, shutter speed and aperture, all writable. */
export function defaultCameraSettings(): CameraSetting[] {
  return [
    { kind: 'iso', value: '100', choices: ['100', '200', '400', '800'], writable: true },
    {
      kind: 'shutter_speed',
      value: '0.0100s',
      choices: ['0.0040s', '0.0100s', '0.0166s', '0.0200s'],
      writable: true,
    },
    { kind: 'aperture', value: '8', choices: ['5.6', '8', '11'], writable: true },
  ];
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
  /** How sure that base was (null: picked by hand, or none). */
  rollBaseConfidence: null as number | null,
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
  /** The areas sampled for a neutral grey, in order. */
  neutralSamples: [] as Array<{
    id: string;
    recipe: Recipe;
    region: { x: number; y: number; w: number; h: number };
  }>,
  /** Sampling a neutral grey is refused with this message when set. */
  neutralRefused: null as string | null,
  /** The settings applied to frames, in order. */
  applies: [] as Array<{ frameIds: string[]; source: string; fields: string[] }>,
  /** Applying settings is refused with this message when set. */
  applyRefused: null as string | null,
  /** The feature flags now, and the settings changes asked for, in order. */
  flags: { ...DEFAULT_FLAGS },
  settingsPatches: [] as SettingsPatchShape[],
  /** The Starting look setting now. */
  startingLook: 'neutral' as 'neutral' | 'film_log' | 'frontier',
  /** The keyboard shortcuts the user changed (action name to key). */
  keys: {} as Record<string, string>,
  /** Reading the settings fails with this message when set. */
  settingsUnreadable: null as string | null,
  /** Saving settings is refused with this message when set. */
  settingsRefused: null as string | null,
  /** What the update check finds, and the message the check or the install fails with when set. */
  updateOutcome: { status: 'up_to_date' } as UpdateOutcome,
  updateCheckFails: null as string | null,
  updateInstallFails: null as string | null,
  /** The counters of the open library; null while no library is open. */
  counters: null as Counters | null,
  /** Where the save dialog sends the diagnostics zip; null when the user cancels. */
  diagnosticsPick: '/home/film/diagnostics.zip' as string | null,
  /** The files the diagnostics were exported to, in order. */
  diagnosticsExports: [] as string[],
  diagnosticsFails: null as string | null,
  /** The exports started, in order. */
  exports: [] as Array<{ frameIds: string[]; options: unknown }>,
  /** Starting an export is refused with this message when set. */
  exportRefused: null as string | null,
  /** How many library checks were started, and starting one is refused with this message when set. */
  verifies: 0,
  verifyRefused: null as string | null,
  /** Whether the frames opened in Develop have an infrared plane. */
  hasIr: false,
  /** The frame the analysis finds, and how sure it is. */
  frameRect: { x: 0.05, y: 0.1, w: 0.9, h: 0.8 },
  frameConfidence: 0.9,
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
  /** What the camera-file picker returns (the files the user chose). */
  pickedCamera: [] as string[],
  /** What scanner_list answers, or the message it fails with. */
  devices: [{ id: 'usb:1', name: 'Nikon LS-50 ED' }] as DeviceInfo[],
  devicesFail: null as string | null,
  /** What scan_resume_point answers. */
  resume: 1,
  /** What scan_start fails with, if it does. */
  scanFail: null as string | null,
  /** What scan_preview answers, or the message it fails with. */
  previewImage: null as RollPreviewImage | null,
  previewFail: null as string | null,
  /** What 	riplet_group answers. */
  triplets: [] as TripletCandidate[],
  tripletsFail: null as string | null,
  /** What camera_list answers, or the message it fails with. */
  cameras: [{ id: 'cam:1', name: 'Nikon D600' }] as CameraInfo[],
  camerasFail: null as string | null,
  /** What camera_settings_get answers (a set changes it), and the messages the two fail with. */
  cameraSettings: defaultCameraSettings(),
  cameraSettingsFail: null as string | null,
  /** The message camera_preview_frame fails with, or null for frames. */
  liveViewFail: null as string | null,
  /** The zoom steps of the live view, whole frame first; empty for a camera without zoom. */
  zoomSteps: [
    { value: '0', label: 'Whole frame' },
    { value: '2', label: '25 %' },
    { value: '4', label: '50 %' },
    { value: '6', label: '100 %' },
    { value: '7', label: '200 %' },
  ] as ZoomStep[],
  cameraSettingsSetFail: null as string | null,
  /** What light_connect answers, or the message it fails with. */
  light: { port: 'COM3', hardware_id: 2, firmware_id: 1, recognised: true } as LightInfo,
  lightFail: null as string | null,
  /** The capture profile, and the message a change of it is refused with. */
  profile: {
    levels: [44, 22, 2],
    settle_ms: 300,
    temp_warn_c: 45,
    temp_abort_c: 60,
    camera_backend: 'gphoto',
    watch_folder: null,
  } as CaptureProfile,
  profileRefused: null as string | null,
  /** What capture_test answers, or the message it fails with. */
  testShot: {
    histogram: new Array(256).fill(0),
    clipped_pct: 0,
    exposure: { exposure_time: { num: 1, den: 60 }, iso: 200, f_number: { num: 56, den: 10 } },
  } as TestShot,
  /** Levels saved per film stock (key: the stock as the roll spells it), and the default's exposure. */
  savedStocks: {} as Record<string, CaptureLevels>,
  savedDefaultExposure: null as CaptureLevels['exposure'],
  levelsFail: null as string | null,
  testFail: null as string | null,
  /** What capture_focus_check answers, or the message it fails with; and the saved flat reference. */
  focusCheck: { ...DEFAULT_FOCUS_CHECK },
  focusFail: null as string | null,
  focusReference: null as FocusCheck['raw_map'] | null,
  /** The id capture_triplet answers, or the message it fails with. */
  captureJob: 'job-capture-1' as string,
  captureFail: null as string | null,
  /** The id capture_calibrate answers, or the message it fails with. */
  calibrationJob: 'job-calibration-1' as string,
  calibrationFail: null as string | null,
};

export const listeners = {
  changed: [] as Array<(roll: string | null, frameIds: string[]) => void>,
  progress: [] as Array<(e: { id: string; stage: string; fraction: number }) => void>,
  done: [] as Array<
    (e: {
      id: string;
      result:
        | { kind: 'Finished' }
        | ({ kind: 'Imported' } & ImportResult)
        | ({ kind: 'Verified' } & VerifyResult);
    }) => void
  >,
  error: [] as Array<(e: { id: string; error: AppError }) => void>,
  step: [] as Array<(e: CaptureStep) => void>,
  light: [] as Array<(e: LightStatus) => void>,
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
  settingsGet: vi.fn(async () => {
    if (state.settingsUnreadable) {
      throw new IpcError({ code: 'Io', message: state.settingsUnreadable });
    }
    return {
      last_library: state.lastLibrary,
      starting_look: state.startingLook,
      flags: { ...state.flags },
      keys: { ...state.keys },
      focus_reference: state.focusReference,
    };
  }),
  settingsDefaults: vi.fn(async () => ({ last_library: null, flags: { ...DEFAULT_FLAGS } })),
  settingsSet: vi.fn(async (patch: SettingsPatchShape) => {
    state.settingsPatches.push(patch);
    if (state.settingsRefused) {
      throw new IpcError({ code: 'Io', message: state.settingsRefused });
    }
    state.flags = { ...state.flags, ...patch.flags };
    if (patch.keys) state.keys = { ...patch.keys };
    if (patch.starting_look) state.startingLook = patch.starting_look;
    if (patch.focus_reference) state.focusReference = patch.focus_reference;
    if (patch.clear_focus_reference) state.focusReference = null;
    return {
      last_library: state.lastLibrary,
      starting_look: state.startingLook,
      flags: { ...state.flags },
      keys: { ...state.keys },
      focus_reference: state.focusReference,
    };
  }),
  updateCheck: vi.fn<(manual: boolean) => Promise<UpdateOutcome>>(async () => {
    if (state.updateCheckFails) throw new IpcError({ code: 'Io', message: state.updateCheckFails });
    return state.flags.update_check ? state.updateOutcome : { status: 'disabled' };
  }),
  updateInstall: vi.fn(async () => {
    if (state.updateInstallFails) {
      throw new IpcError({ code: 'InvalidInput', message: state.updateInstallFails });
    }
  }),
  updateRestart: vi.fn(async () => undefined),
  countersGet: vi.fn(async () => state.counters),
  pickDiagnosticsFile: vi.fn(async () => state.diagnosticsPick),
  diagnosticsExport: vi.fn(async (dest: string) => {
    if (state.diagnosticsFails) {
      throw new IpcError({ code: 'Io', message: state.diagnosticsFails });
    }
    state.diagnosticsExports.push(dest);
  }),
  folderCheck: vi.fn(async (path: string): Promise<FolderCheck> => ({
    path,
    free_bytes: 54_900_000_000,
    ...state.folder,
  })),
  pickFolder: vi.fn(async () => state.picked),
  pickTiffFiles: vi.fn(async (): Promise<string[]> => []),
  pickCameraFiles: vi.fn(async (): Promise<string[]> => [...state.pickedCamera]),
  tripletGroup: vi.fn(async (paths: string[]): Promise<TripletCandidate[]> => {
    void paths;
    if (state.tripletsFail) throw new IpcError({ code: 'Io', message: state.tripletsFail });
    return state.triplets;
  }),
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
      has_ir: state.hasIr,
      recipe: makeRecipe('saved'),
      roll_base: state.rollBase,
      roll_base_confidence: state.rollBaseConfidence,
    };
  }),
  developAnalyze: vi.fn(
    async (id: string, base: [number, number, number] | null, look: Look | null = null) => {
      if (state.noAnalysis.has(id)) {
        throw new IpcError({ code: 'InvalidInput', message: 'The scan could not be analysed.' });
      }
      // The levels depend on the base that was given, as the real analysis's do.
      return {
        frame_rect: state.frameRect,
        frame_confidence: state.frameConfidence,
        auto_recipe: makeRecipe('auto', {
          base: { source: 'roll', rgb: base ?? [1, 1, 1] },
          invert: base
            ? { black: [0.01, 0.02, 0.03], white: [0.5, 0.6, 0.7] }
            : { black: [0.05, 0.06, 0.07], white: [0.95, 0.85, 0.75] },
          // A look writes its master curve; Neutral leaves the plain line.
          ...((look ?? state.startingLook) === 'film_log'
            ? { tone: { curve: { master: FILM_LOG_CURVE } } }
            : {}),
          // Frontier writes a curve and a saturation.
          ...((look ?? state.startingLook) === 'frontier'
            ? { tone: { curve: { master: FRONTIER_CURVE } }, color: { saturation: 1.1 } }
            : {}),
        }),
      };
    },
  ),
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
  developSampleNeutral: vi.fn(
    async (id: string, recipe: Recipe, region: { x: number; y: number; w: number; h: number }) => {
      state.neutralSamples.push({ id, recipe, region });
      if (state.neutralRefused) {
        throw new IpcError({ code: 'InvalidInput', message: state.neutralRefused });
      }
      const current = (recipe as unknown as { color?: Record<string, number> }).color ?? {};
      return { ...current, red_cyan: -20, green_magenta: 10, blue_yellow: 30 };
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
  rollSetFrameSize: vi.fn(async (id: string, size: [number, number] | null) => {
    state.rolls = state.rolls.map((r) =>
      r.id === id ? { ...r, frame_size: size && { w: size[0], h: size[1], samples: 0 } } : r,
    );
    return state.rolls.find((r) => r.id === id) as Roll;
  }),
  flatFieldImport: vi.fn(async (id: string, paths: string[]) => {
    state.rolls = state.rolls.map((r) => (r.id === id ? { ...r, has_flat_field: true } : r));
    return { frame_width: 6016, frame_height: 4016, sources: paths };
  }),
  flatFieldClear: vi.fn(async (id: string) => {
    const had = state.rolls.find((r) => r.id === id)?.has_flat_field ?? false;
    state.rolls = state.rolls.map((r) => (r.id === id ? { ...r, has_flat_field: false } : r));
    return had;
  }),
  logClientEvent: vi.fn(async () => undefined),
  developRender: vi.fn(async (_id: string, recipe: Recipe, maxEdge: number, seq: number) =>
    makePreview(recipe, maxEdge, seq),
  ),
  frameThumbnail: vi.fn(async (id: string) => {
    if (state.noThumb.has(id)) throw new IpcError({ code: 'NotFound', message: 'no thumbnail' });
    return new Uint8Array([0xff, 0xd8, 1, 2, 3]).buffer;
  }),
  cameraPreviewFrame: vi.fn(async () => {
    if (state.liveViewFail) throw new IpcError({ code: 'Internal', message: state.liveViewFail });
    // Yields like a real round trip, so a loop cannot starve the test.
    await new Promise((resolve) => setTimeout(resolve, 1));
    return new Uint8Array([0xff, 0xd8, 4, 5, 6]).buffer;
  }),
  cameraPreviewStop: vi.fn(async () => undefined),
  cameraPreviewZoomSteps: vi.fn(async (): Promise<ZoomStep[]> =>
    state.zoomSteps.map((s) => ({ ...s })),
  ),
  cameraPreviewSetZoom: vi.fn(async (value: string) => {
    void value;
  }),
  cameraPreviewSetArea: vi.fn(async (x: number, y: number) => {
    void [x, y];
  }),
  importFiles: vi.fn(async (paths: string[], roll: string, kind: string) => {
    void [paths, roll, kind];
    return 'job-1';
  }),
  recipeApplyTo: vi.fn(async (frameIds: string[], source: string, fields: string[]) => {
    state.applies.push({ frameIds, source, fields });
    if (state.applyRefused) {
      throw new IpcError({ code: 'InvalidInput', message: state.applyRefused });
    }
  }),
  exportStart: vi.fn(async (frameIds: string[], options: unknown) => {
    state.exports.push({ frameIds, options });
    if (state.exportRefused) {
      throw new IpcError({ code: 'InvalidInput', message: state.exportRefused });
    }
    return 'job-export-1';
  }),
  libraryVerify: vi.fn(async () => {
    state.verifies += 1;
    if (state.verifyRefused) {
      throw new IpcError({ code: 'NotFound', message: state.verifyRefused });
    }
    return 'job-verify-1';
  }),
  scannerList: vi.fn(async (): Promise<DeviceInfo[]> => {
    if (state.devicesFail) {
      throw new IpcError({ code: 'ScannerUnavailable', message: state.devicesFail });
    }
    return [...state.devices];
  }),
  scanResumePoint: vi.fn(async (roll: string) => {
    void roll;
    return state.resume;
  }),
  scanStart: vi.fn(async (roll: string, request: ScanRequest) => {
    void [roll, request];
    if (state.scanFail) throw new IpcError({ code: 'ScannerUnavailable', message: state.scanFail });
    return 'job-scan';
  }),
  scanPreview: vi.fn(async (device: string): Promise<RollPreviewImage> => {
    void device;
    if (state.previewFail) throw new IpcError({ code: 'ScanFailed', message: state.previewFail });
    return (
      state.previewImage ?? {
        frame_count: 4,
        pitch: 100,
        strip_width: 400,
        strip_height: 96,
        strip_jpeg: [0xff, 0xd8, 0xff, 0xd9],
      }
    );
  }),
  scanPreviewCancel: vi.fn(async () => true),
  jobCancel: vi.fn(async () => true),
  lightList: vi.fn(async () => []),
  lightConnect: vi.fn(async (port: string | null): Promise<LightInfo> => {
    void port;
    if (state.lightFail) throw new IpcError({ code: 'Internal', message: state.lightFail });
    return { ...state.light };
  }),
  lightSet: vi.fn(async () => undefined),
  lightOff: vi.fn(async () => undefined),
  cameraList: vi.fn(async (): Promise<CameraInfo[]> => {
    if (state.camerasFail) throw new IpcError({ code: 'Internal', message: state.camerasFail });
    return [...state.cameras];
  }),
  cameraSettingsGet: vi.fn(async (): Promise<CameraSetting[]> => {
    if (state.cameraSettingsFail) {
      throw new IpcError({ code: 'Io', message: state.cameraSettingsFail });
    }
    return state.cameraSettings.map((s) => ({ ...s }));
  }),
  cameraSettingsSet: vi.fn(async (kind: CameraSettingKind, value: string) => {
    if (state.cameraSettingsSetFail) {
      throw new IpcError({ code: 'InvalidInput', message: state.cameraSettingsSetFail });
    }
    const setting = state.cameraSettings.find((s) => s.kind === kind);
    if (!setting?.writable || !setting.choices.includes(value)) {
      throw new IpcError({
        code: 'InvalidInput',
        message: 'That value is not one the camera offers.',
      });
    }
    setting.value = value;
    return state.cameraSettings.map((s) => ({ ...s }));
  }),
  captureProfileGet: vi.fn(async (): Promise<CaptureProfile> => ({ ...state.profile })),
  captureProfileSet: vi.fn(async (patch: { levels?: [number, number, number] | null }) => {
    if (state.profileRefused) {
      throw new IpcError({ code: 'InvalidInput', message: state.profileRefused });
    }
    state.profile = { ...state.profile, levels: patch.levels ?? state.profile.levels };
    return { ...state.profile };
  }),
  captureLevelsGet: vi.fn(async (rollId: string): Promise<CaptureLevels> => {
    if (state.levelsFail) throw new IpcError({ code: 'Internal', message: state.levelsFail });
    const stock = state.rolls
      .find((r) => r.id === rollId)
      ?.film_stock?.trim()
      .toLowerCase();
    const hit = stock ? state.savedStocks[stock] : undefined;
    return hit
      ? { ...hit }
      : {
          levels: [...state.profile.levels] as [number, number, number],
          source: 'default',
          stock: null,
          exposure: state.savedDefaultExposure,
        };
  }),
  captureLevelsSave: vi.fn(
    async (
      scope: LevelsScope,
      levels: [number, number, number],
      exposure: CaptureLevels['exposure'],
    ): Promise<CaptureLevels> => {
      if (state.levelsFail) throw new IpcError({ code: 'Internal', message: state.levelsFail });
      if (scope === 'default') {
        state.profile = { ...state.profile, levels };
        state.savedDefaultExposure = exposure;
        return { levels, source: 'default', stock: null, exposure };
      }
      const saved: CaptureLevels = { levels, source: 'stock', stock: scope.stock, exposure };
      state.savedStocks[scope.stock.trim().toLowerCase()] = saved;
      return saved;
    },
  ),
  captureTest: vi.fn(
    async (channel: string, levels?: [number, number, number]): Promise<TestShot> => {
      void channel;
      void levels;
      if (state.testFail) throw new IpcError({ code: 'Internal', message: state.testFail });
      return { ...state.testShot };
    },
  ),
  captureFocusCheck: vi.fn(async (channel: string, levels?: [number, number, number]) => {
    void channel;
    void levels;
    if (state.focusFail) throw new IpcError({ code: 'Internal', message: state.focusFail });
    return { ...state.focusCheck };
  }),
  captureTriplet: vi.fn(
    async (rollId: string, levels?: [number, number, number]): Promise<string> => {
      void rollId;
      void levels;
      if (state.captureFail) throw new IpcError({ code: 'Internal', message: state.captureFail });
      return state.captureJob;
    },
  ),
  captureCalibrate: vi.fn(
    async (rollId: string, levels?: [number, number, number]): Promise<string> => {
      void rollId;
      void levels;
      if (state.calibrationFail) {
        throw new IpcError({ code: 'InvalidInput', message: state.calibrationFail });
      }
      return state.calibrationJob;
    },
  ),
  onLibraryChanged: vi.fn((cb: (roll: string | null, frameIds: string[]) => void) =>
    subscribe(listeners.changed, cb),
  ),
  onJobProgress: vi.fn((cb: (typeof listeners.progress)[number]) =>
    subscribe(listeners.progress, cb),
  ),
  onJobDone: vi.fn((cb: (typeof listeners.done)[number]) => subscribe(listeners.done, cb)),
  onJobError: vi.fn((cb: (typeof listeners.error)[number]) => subscribe(listeners.error, cb)),
  onCaptureStep: vi.fn((cb: (typeof listeners.step)[number]) => subscribe(listeners.step, cb)),
  onLightStatus: vi.fn((cb: (typeof listeners.light)[number]) => subscribe(listeners.light, cb)),
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
  state.rollBaseConfidence = null;
  state.saved = [];
  state.samples = [];
  state.levelSamples = [];
  state.levelsRefused = null;
  state.frameRect = { x: 0.05, y: 0.1, w: 0.9, h: 0.8 };
  state.frameConfidence = 0.9;
  state.exportRefused = null;
  state.verifies = 0;
  state.hasIr = false;
  state.verifyRefused = null;
  state.exports = [];
  state.flags = { ...DEFAULT_FLAGS };
  state.keys = {};
  state.settingsPatches = [];
  state.startingLook = 'neutral';
  state.settingsUnreadable = null;
  state.settingsRefused = null;
  state.updateOutcome = { status: 'up_to_date' };
  state.updateCheckFails = null;
  state.updateInstallFails = null;
  state.counters = null;
  state.diagnosticsPick = '/home/film/diagnostics.zip';
  state.diagnosticsExports = [];
  state.diagnosticsFails = null;
  state.applies = [];
  state.applyRefused = null;
  state.neutralSamples = [];
  state.neutralRefused = null;
  state.saveFails = null;
  state.rollBases = [];
  state.tags = [];
  state.frameTags = {};
  state.rollTags = {};
  state.picked = '/films/Library';
  state.pickedCamera = [];
  state.devices = [{ id: 'usb:1', name: 'Nikon LS-50 ED' }];
  state.devicesFail = null;
  state.resume = 1;
  state.scanFail = null;
  state.previewImage = null;
  state.previewFail = null;
  state.triplets = [];
  state.tripletsFail = null;
  state.cameras = [{ id: 'cam:1', name: 'Nikon D600' }];
  state.camerasFail = null;
  state.cameraSettings = defaultCameraSettings();
  state.cameraSettingsFail = null;
  state.liveViewFail = null;
  state.zoomSteps = [
    { value: '0', label: 'Whole frame' },
    { value: '2', label: '25 %' },
    { value: '4', label: '50 %' },
    { value: '6', label: '100 %' },
    { value: '7', label: '200 %' },
  ];
  state.cameraSettingsSetFail = null;
  state.light = { port: 'COM3', hardware_id: 2, firmware_id: 1, recognised: true };
  state.lightFail = null;
  state.profile = {
    levels: [44, 22, 2],
    settle_ms: 300,
    temp_warn_c: 45,
    temp_abort_c: 60,
    camera_backend: 'gphoto',
    watch_folder: null,
  } as CaptureProfile;
  state.profileRefused = null;
  state.testShot = {
    histogram: new Array(256).fill(0),
    clipped_pct: 0,
    exposure: { exposure_time: { num: 1, den: 60 }, iso: 200, f_number: { num: 56, den: 10 } },
  };
  state.savedStocks = {};
  state.savedDefaultExposure = null;
  state.levelsFail = null;
  state.testFail = null;
  state.focusFail = null;
  state.focusReference = null;
  state.focusCheck = { ...DEFAULT_FOCUS_CHECK };
  state.captureJob = 'job-capture-1';
  state.captureFail = null;
  state.calibrationJob = 'job-calibration-1';
  state.calibrationFail = null;
  listeners.step.length = 0;
  listeners.light.length = 0;
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
