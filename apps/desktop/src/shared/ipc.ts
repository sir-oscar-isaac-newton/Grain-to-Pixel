// The only place that talks to the Tauri backend. It wraps the commands generated from Rust
// (packages/ipc-types) and turns the `{ status: 'error' }` results into thrown `IpcError`s.
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  events,
  commands,
  type AppError,
  type ClientLogLevel,
  type CrashNote,
  type ImportResult,
  type LibraryDetails,
  type LibraryInfo,
  type Settings,
  type SettingsPatch,
  type SkipReason,
  type SkippedFile,
} from '@gtp/ipc-types';

export type {
  AppError,
  ClientLogLevel,
  CrashNote,
  ImportResult,
  LibraryDetails,
  LibraryInfo,
  Settings,
  SettingsPatch,
  SkipReason,
  SkippedFile,
};

export class IpcError extends Error {
  readonly error: AppError;

  constructor(error: AppError) {
    super(error.message);
    this.name = 'IpcError';
    this.error = error;
  }
}

type Result<T> = { status: 'ok'; data: T } | { status: 'error'; error: AppError };

export function unwrap<T>(result: Result<T>): T {
  if (result.status === 'ok') return result.data;
  throw new IpcError(result.error);
}

export async function libraryInfo(): Promise<LibraryInfo> {
  return unwrap(await commands.libraryInfo());
}

/** Creates a library in an empty (or new) folder and opens it. */
export async function libraryCreate(path: string, name: string): Promise<LibraryInfo> {
  return unwrap(await commands.libraryCreate(path, name));
}

/** Opens a library; one made by a newer app version comes back with `read_only: true`. */
export async function libraryOpen(path: string): Promise<LibraryInfo> {
  return unwrap(await commands.libraryOpen(path));
}

/**
 * Starts verifying every original of the open library; returns the job id at once. Progress and
 * the `VerifyResult` arrive as `job://*` events, and `jobCancel` stops it. It only reports.
 */
export async function libraryVerify(): Promise<Data<'libraryVerify'>> {
  return unwrap(await commands.libraryVerify());
}

export async function settingsGet(): Promise<Settings> {
  return unwrap(await commands.settingsGet());
}

/** Changes the named settings and returns all of them. */
export async function settingsSet(patch: SettingsPatch): Promise<Settings> {
  return unwrap(await commands.settingsSet(patch));
}

// Rolls and frames. Argument and result types come straight from the generated commands.
type Args<K extends keyof typeof commands> = Parameters<(typeof commands)[K]>;
type Data<K extends keyof typeof commands> =
  Awaited<ReturnType<(typeof commands)[K]>> extends Result<infer T> ? T : never;

export type RollInput = Args<'rollCreate'>[0];
export type FrameQuery = Args<'frameList'>[0];
export type Roll = Data<'rollCreate'>;
export type Frame = Data<'frameGet'>;
export type FramePage = Data<'frameList'>;
export type DeleteReport = Data<'rollDelete'>;

export async function rollCreate(input: RollInput): Promise<Roll> {
  return unwrap(await commands.rollCreate(input));
}

/** Replaces every editable field of the roll; an empty field is cleared. */
export async function rollUpdate(...args: Args<'rollUpdate'>): Promise<Roll> {
  return unwrap(await commands.rollUpdate(...args));
}

export async function rollList(): Promise<Roll[]> {
  return unwrap(await commands.rollList());
}

/** Sets the roll's film base, or clears it with `null`. */
export async function rollSetFilmBase(...args: Args<'rollSetFilmBase'>): Promise<Roll> {
  return unwrap(await commands.rollSetFilmBase(...args));
}

/** Deletes a roll and its frames; `files_left` lists files that could not be removed. */
export async function rollDelete(...args: Args<'rollDelete'>): Promise<DeleteReport> {
  return unwrap(await commands.rollDelete(...args));
}

export async function frameList(query: FrameQuery): Promise<FramePage> {
  return unwrap(await commands.frameList(query));
}

export async function frameGet(...args: Args<'frameGet'>): Promise<Frame> {
  return unwrap(await commands.frameGet(...args));
}

export async function frameSetNote(...args: Args<'frameSetNote'>): Promise<Frame> {
  return unwrap(await commands.frameSetNote(...args));
}

/** Deletes frames; `files_left` lists files that could not be removed. */
export async function frameDelete(...args: Args<'frameDelete'>): Promise<DeleteReport> {
  return unwrap(await commands.frameDelete(...args));
}

/**
 * Starts importing files into a roll; returns the job id at once. Progress and the
 * `ImportResult` arrive as `job://*` events, each new frame as `library://changed`.
 */
export async function importFiles(...args: Args<'importFiles'>): Promise<Data<'importFiles'>> {
  return unwrap(await commands.importFiles(...args));
}

// Tags and search.
export type SearchQuery = Args<'search'>[0];
export type Tag = Data<'tagRename'>;

/** All tags, in name order. */
export async function tagList(): Promise<Data<'tagList'>> {
  return unwrap(await commands.tagList());
}

/** The tags of a roll, in name order. */
export async function rollTags(...args: Args<'rollTags'>): Promise<Tag[]> {
  return unwrap(await commands.rollTags(...args));
}

/** The tags on any of the frames themselves (not from their rolls), each once, in name order. */
export async function framesOwnTags(...args: Args<'framesOwnTags'>): Promise<Tag[]> {
  return unwrap(await commands.framesOwnTags(...args));
}

/**
 * Adds the tags named in `add` (created when missing) to the frames and removes those named in
 * `remove`, all or nothing.
 */
export async function tagApply(...args: Args<'tagApply'>): Promise<void> {
  unwrap(await commands.tagApply(...args));
}

/** The same for a roll: its tags count for every frame on it, also frames added later. */
export async function tagApplyRoll(...args: Args<'tagApplyRoll'>): Promise<void> {
  unwrap(await commands.tagApplyRoll(...args));
}

export async function tagRename(...args: Args<'tagRename'>): Promise<Data<'tagRename'>> {
  return unwrap(await commands.tagRename(...args));
}

export async function tagDelete(...args: Args<'tagDelete'>): Promise<void> {
  unwrap(await commands.tagDelete(...args));
}

/**
 * One page of the frames matching `query` (every part optional, all must match): best text match
 * first when there is text, otherwise newest roll first. `page` counts from 0.
 */
export async function search(...args: Args<'search'>): Promise<FramePage> {
  return unwrap(await commands.search(...args));
}

/** Crashes of earlier runs; the backend hands them out once. */
export async function crashNotesTake(): Promise<CrashNote[]> {
  return unwrap(await commands.crashNotesTake());
}

/** Sends an event to the application log. Never throws: reporting an error must not cause one. */
export async function logClientEvent(level: ClientLogLevel, message: string): Promise<void> {
  try {
    await commands.logClientEvent(level, message.slice(0, MAX_CLIENT_MESSAGE));
  } catch {
    // No backend (tests, plain browser) or a failed call: there is nowhere else to report it.
  }
}

// Keeps a message under the backend limit (2000 characters) with room for the ellipsis.
const MAX_CLIENT_MESSAGE = 1990;

// Folder checks, thumbnails, pickers, job control and events for the Library view.
export type FolderCheck = Data<'folderCheck'>;
export type FrameSummary = FramePage['items'][number];

/** What the first-run dialog shows about a folder. Nothing is created. */
export async function folderCheck(path: string): Promise<FolderCheck> {
  return unwrap(await commands.folderCheck(path));
}

/** Stops a running job; false when it is not running (already finished), which is harmless. */
export async function jobCancel(id: string): Promise<boolean> {
  return unwrap(await commands.jobCancel(id));
}

function isAppError(value: unknown): value is AppError {
  return (
    typeof value === 'object' &&
    value !== null &&
    'code' in value &&
    'message' in value &&
    typeof (value as { message: unknown }).message === 'string'
  );
}

/**
 * A frame's thumbnail as JPEG bytes. This command returns raw bytes instead of JSON, so it is not
 * in the generated bindings (see `ipc::RAW_COMMANDS` in the shell); errors still arrive as
 * `AppError`s.
 */
export async function frameThumbnail(id: string): Promise<ArrayBuffer> {
  try {
    return await invoke<ArrayBuffer>('frame_thumbnail', { id });
  } catch (error) {
    if (isAppError(error)) throw new IpcError(error);
    throw error;
  }
}

/** Opens the system folder picker; null when cancelled. */
export async function pickFolder(): Promise<string | null> {
  const chosen = await openDialog({ directory: true, multiple: false });
  return typeof chosen === 'string' ? chosen : null;
}

/** Opens the system file picker for TIFF scans; an empty list when cancelled. */
export async function pickTiffFiles(): Promise<string[]> {
  const chosen = await openDialog({
    multiple: true,
    directory: false,
    filters: [{ name: 'TIFF', extensions: ['tif', 'tiff'] }],
  });
  if (chosen === null) return [];
  return Array.isArray(chosen) ? chosen : [chosen];
}

/** Calls `cb` when frames were added, changed or removed. Returns a function that stops listening. */
export async function onLibraryChanged(cb: (rollId: string | null) => void): Promise<() => void> {
  return events.libraryChanged.listen((event) => cb(event.payload.roll_id ?? null));
}

export interface JobProgressEvent {
  id: string;
  stage: string;
  fraction: number;
}

export async function onJobProgress(cb: (e: JobProgressEvent) => void): Promise<() => void> {
  return events.jobProgress.listen((event) =>
    cb({ ...event.payload, fraction: event.payload.fraction ?? 0 }),
  );
}

type JobDoneEvent = Parameters<Parameters<typeof events.jobDone.listen>[0]>[0]['payload'];
type JobErrorEvent = Parameters<Parameters<typeof events.jobError.listen>[0]>[0]['payload'];
export type JobResult = JobDoneEvent['result'];

export async function onJobDone(cb: (e: JobDoneEvent) => void): Promise<() => void> {
  return events.jobDone.listen((event) => cb(event.payload));
}

export async function onJobError(cb: (e: JobErrorEvent) => void): Promise<() => void> {
  return events.jobError.listen((event) => cb(event.payload));
}
