import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();
const openMock = vi.fn();
const folderCheckMock = vi.fn();
const jobCancelMock = vi.fn();
const handlers: Record<string, (event: { payload: unknown }) => void> = {};
const stop = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: (...args: unknown[]) => openMock(...args),
}));
vi.mock('@gtp/ipc-types', () => {
  const listener = (name: string) => ({
    listen: (cb: (event: { payload: unknown }) => void) => {
      handlers[name] = cb;
      return Promise.resolve(stop);
    },
  });
  return {
    commands: {
      folderCheck: (path: string) => folderCheckMock(path),
      jobCancel: (id: string) => jobCancelMock(id),
    },
    events: {
      libraryChanged: listener('libraryChanged'),
      jobProgress: listener('jobProgress'),
      jobDone: listener('jobDone'),
      jobError: listener('jobError'),
    },
  };
});

import {
  IpcError,
  folderCheck,
  frameThumbnail,
  jobCancel,
  onJobDone,
  onJobError,
  onJobProgress,
  onLibraryChanged,
  pickFolder,
  pickTiffFiles,
} from './ipc';

describe('shared/ipc: library helpers', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    openMock.mockReset();
    folderCheckMock.mockReset();
    jobCancelMock.mockReset();
    stop.mockReset();
  });

  describe('frameThumbnail', () => {
    it('calls the raw-bytes command by name and returns the bytes', async () => {
      const bytes = new Uint8Array([0xff, 0xd8]).buffer;
      invokeMock.mockResolvedValue(bytes);
      await expect(frameThumbnail('f1')).resolves.toBe(bytes);
      expect(invokeMock).toHaveBeenCalledWith('frame_thumbnail', { id: 'f1' });
    });

    it('turns the backend error into an IpcError, and passes other failures through', async () => {
      const error = { code: 'NotFound', message: 'That thumbnail does not exist.' };
      invokeMock.mockRejectedValueOnce(error);
      const failure = frameThumbnail('f1');
      await expect(failure).rejects.toBeInstanceOf(IpcError);
      await expect(failure).rejects.toMatchObject({ error, message: error.message });

      invokeMock.mockRejectedValueOnce(new TypeError('no ipc'));
      await expect(frameThumbnail('f1')).rejects.toThrow('no ipc');
      invokeMock.mockRejectedValueOnce('a plain string');
      await expect(frameThumbnail('f1')).rejects.toBe('a plain string');
    });
  });

  describe('pickers', () => {
    it('picks a folder, or nothing when cancelled', async () => {
      openMock.mockResolvedValueOnce('/films/Library');
      await expect(pickFolder()).resolves.toBe('/films/Library');
      expect(openMock).toHaveBeenCalledWith({ directory: true, multiple: false });
      openMock.mockResolvedValueOnce(null);
      await expect(pickFolder()).resolves.toBeNull();
    });

    it('picks TIFF files: a list, a single path as a list, and an empty list when cancelled', async () => {
      openMock.mockResolvedValueOnce(['/a.tif', '/b.tif']);
      await expect(pickTiffFiles()).resolves.toEqual(['/a.tif', '/b.tif']);
      expect(openMock).toHaveBeenCalledWith({
        multiple: true,
        directory: false,
        filters: [{ name: 'TIFF', extensions: ['tif', 'tiff'] }],
      });
      openMock.mockResolvedValueOnce('/only.tif');
      await expect(pickTiffFiles()).resolves.toEqual(['/only.tif']);
      openMock.mockResolvedValueOnce(null);
      await expect(pickTiffFiles()).resolves.toEqual([]);
    });
  });

  describe('commands', () => {
    it('checks a folder and cancels a job through the typed commands', async () => {
      folderCheckMock.mockResolvedValue({ status: 'ok', data: { path: '/x', free_bytes: 5 } });
      await expect(folderCheck('/x')).resolves.toMatchObject({ path: '/x' });
      expect(folderCheckMock).toHaveBeenCalledWith('/x');

      jobCancelMock.mockResolvedValue({ status: 'ok', data: false });
      await expect(jobCancel('job-1')).resolves.toBe(false);
      folderCheckMock.mockResolvedValue({
        status: 'error',
        error: { code: 'InvalidInput', message: 'That is not a folder.' },
      });
      await expect(folderCheck('/file')).rejects.toBeInstanceOf(IpcError);
    });
  });

  describe('events', () => {
    it('reports which roll changed, or null for several', async () => {
      const seen: Array<string | null> = [];
      const unlisten = await onLibraryChanged((roll) => seen.push(roll));
      handlers.libraryChanged?.({ payload: { roll_id: 'r1' } });
      handlers.libraryChanged?.({ payload: { roll_id: null } });
      handlers.libraryChanged?.({ payload: {} });
      expect(seen).toEqual(['r1', null, null]);
      unlisten();
      expect(stop).toHaveBeenCalledTimes(1);
    });

    it('treats a progress without a fraction as zero', async () => {
      const seen: number[] = [];
      await onJobProgress((e) => seen.push(e.fraction));
      handlers.jobProgress?.({ payload: { id: 'j', stage: 's', fraction: 0.4 } });
      handlers.jobProgress?.({ payload: { id: 'j', stage: 's', fraction: null } });
      expect(seen).toEqual([0.4, 0]);
    });

    it('passes job results and errors on', async () => {
      const done = vi.fn();
      const failed = vi.fn();
      await onJobDone(done);
      await onJobError(failed);
      handlers.jobDone?.({ payload: { id: 'j', result: { kind: 'Finished' } } });
      handlers.jobError?.({ payload: { id: 'j', error: { code: 'Io', message: 'x' } } });
      expect(done).toHaveBeenCalledWith({ id: 'j', result: { kind: 'Finished' } });
      expect(failed).toHaveBeenCalledWith({ id: 'j', error: { code: 'Io', message: 'x' } });
    });
  });
});
