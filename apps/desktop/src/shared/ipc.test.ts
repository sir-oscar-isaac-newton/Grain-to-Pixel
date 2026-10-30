import { beforeEach, describe, expect, it, vi } from 'vitest';

const libraryInfoMock = vi.fn();
const logClientEventMock = vi.fn();
vi.mock('@gtp/ipc-types', () => ({
  commands: {
    libraryInfo: () => libraryInfoMock(),
    logClientEvent: (level: string, message: string) => logClientEventMock(level, message),
  },
}));

import { IpcError, libraryInfo, logClientEvent, unwrap } from './ipc';

describe('shared/ipc', () => {
  beforeEach(() => {
    libraryInfoMock.mockReset();
    logClientEventMock.mockReset();
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
    libraryInfoMock.mockResolvedValue({ status: 'ok', data: { app_version: '9.9.9' } });
    await expect(libraryInfo()).resolves.toEqual({ app_version: '9.9.9' });
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
