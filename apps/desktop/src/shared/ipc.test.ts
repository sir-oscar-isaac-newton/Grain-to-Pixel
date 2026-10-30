import { beforeEach, describe, expect, it, vi } from 'vitest';

const libraryInfoMock = vi.fn();
vi.mock('@gtp/ipc-types', () => ({
  commands: { libraryInfo: () => libraryInfoMock() },
}));

import { IpcError, libraryInfo, unwrap } from './ipc';

describe('shared/ipc', () => {
  beforeEach(() => {
    libraryInfoMock.mockReset();
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
