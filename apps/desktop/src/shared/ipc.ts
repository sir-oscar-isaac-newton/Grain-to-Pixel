// The only place that talks to the Tauri backend. It wraps the commands generated from Rust
// (packages/ipc-types) and turns the `{ status: 'error' }` results into thrown `IpcError`s.
import { commands, type AppError, type LibraryInfo } from '@gtp/ipc-types';

export type { AppError, LibraryInfo };

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
