// The only place that talks to the Tauri backend. It wraps the commands generated from Rust
// (packages/ipc-types) and turns the `{ status: 'error' }` results into thrown `IpcError`s.
import {
  commands,
  type AppError,
  type ClientLogLevel,
  type CrashNote,
  type LibraryInfo,
} from '@gtp/ipc-types';

export type { AppError, ClientLogLevel, CrashNote, LibraryInfo };

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
