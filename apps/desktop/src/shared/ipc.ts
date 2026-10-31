// The only place that talks to the Tauri backend. It wraps the commands generated from Rust
// (packages/ipc-types) and turns the `{ status: 'error' }` results into thrown `IpcError`s.
import {
  commands,
  type AppError,
  type ClientLogLevel,
  type CrashNote,
  type LibraryDetails,
  type LibraryInfo,
  type Settings,
  type SettingsPatch,
} from '@gtp/ipc-types';

export type {
  AppError,
  ClientLogLevel,
  CrashNote,
  LibraryDetails,
  LibraryInfo,
  Settings,
  SettingsPatch,
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
