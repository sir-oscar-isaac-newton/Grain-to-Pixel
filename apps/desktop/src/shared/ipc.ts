// The only place that talks to the Tauri backend. D-02 replaces these hand-written
// declarations with types generated from Rust (packages/ipc-types).
import { invoke } from '@tauri-apps/api/core';

export interface LibraryInfo {
  app_version: string;
}

export function libraryInfo(): Promise<LibraryInfo> {
  return invoke<LibraryInfo>('library_info');
}
