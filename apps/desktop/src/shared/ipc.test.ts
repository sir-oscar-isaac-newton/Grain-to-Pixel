import { beforeEach, describe, expect, it, vi } from 'vitest';

const libraryInfoMock = vi.fn();
const logClientEventMock = vi.fn();
const libraryOpenMock = vi.fn();
const settingsSetMock = vi.fn();
vi.mock('@gtp/ipc-types', () => ({
  commands: {
    libraryInfo: () => libraryInfoMock(),
    logClientEvent: (level: string, message: string) => logClientEventMock(level, message),
    libraryOpen: (path: string) => libraryOpenMock(path),
    settingsSet: (patch: unknown) => settingsSetMock(patch),
  },
}));

import { IpcError, libraryInfo, libraryOpen, logClientEvent, settingsSet, unwrap } from './ipc';

describe('shared/ipc', () => {
  beforeEach(() => {
    libraryInfoMock.mockReset();
    logClientEventMock.mockReset();
    libraryOpenMock.mockReset();
    settingsSetMock.mockReset();
  });

  it('libraryOpen passes the path and surfaces a read-only library', async () => {
    const library = { path: 'D:\\Film', name: 'Film', read_only: true };
    libraryOpenMock.mockResolvedValue({ status: 'ok', data: { app_version: '1', library } });
    await expect(libraryOpen('D:\\Film')).resolves.toMatchObject({ library: { read_only: true } });
    expect(libraryOpenMock).toHaveBeenCalledWith('D:\\Film');
  });

  it('settingsSet sends only the patch and returns the new settings', async () => {
    settingsSetMock.mockResolvedValue({ status: 'ok', data: { theme: 'dark' } });
    await expect(settingsSet({ theme: 'dark' })).resolves.toEqual({ theme: 'dark' });
    expect(settingsSetMock).toHaveBeenCalledWith({ theme: 'dark' });
  });

  it('logClientEvent passes level and a bounded message to the backend', async () => {
    logClientEventMock.mockResolvedValue({ status: 'ok', data: null });
    await logClientEvent('Warn', 'x'.repeat(5000));
    expect(logClientEventMock).toHaveBeenCalledWith('Warn', 'x'.repeat(1990));
  });

  it('logClientEvent never throws, even without a backend', async () => {
    logClientEventMock.mockRejectedValue(new Error('no backend'));
    await expect(logClientEvent('Error', 'boom')).resolves.toBeUndefined();
  });

  it('returns the data of an ok result', async () => {
    libraryInfoMock.mockResolvedValue({
      status: 'ok',
      data: { app_version: '9.9.9', library: null },
    });
    await expect(libraryInfo()).resolves.toEqual({ app_version: '9.9.9', library: null });
  });

  it('throws an IpcError carrying the AppError of an error result', async () => {
    const error = { code: 'Internal', message: 'boom', details: 'x' } as const;
    libraryInfoMock.mockResolvedValue({ status: 'error', error });
    const failure = libraryInfo();
    await expect(failure).rejects.toBeInstanceOf(IpcError);
    await expect(failure).rejects.toMatchObject({ message: 'boom', error });
  });

  it('unwrap passes ok data through and rejects errors', () => {
    expect(unwrap({ status: 'ok', data: 1 })).toBe(1);
    expect(() => unwrap({ status: 'error', error: { code: 'Io', message: 'disk' } })).toThrow(
      'disk',
    );
  });
});
