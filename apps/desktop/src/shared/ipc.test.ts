import { beforeEach, describe, expect, it, vi } from 'vitest';

const libraryInfoMock = vi.fn();
const logClientEventMock = vi.fn();
const libraryOpenMock = vi.fn();
const settingsSetMock = vi.fn();
const frameDeleteMock = vi.fn();
const searchMock = vi.fn();
const libraryVerifyMock = vi.fn();
const tagApplyMock = vi.fn();
vi.mock('@gtp/ipc-types', () => ({
  commands: {
    libraryInfo: () => libraryInfoMock(),
    logClientEvent: (level: string, message: string) => logClientEventMock(level, message),
    libraryOpen: (path: string) => libraryOpenMock(path),
    settingsSet: (patch: unknown) => settingsSetMock(patch),
    frameDelete: (ids: string[]) => frameDeleteMock(ids),
    libraryVerify: () => libraryVerifyMock(),
    search: (query: unknown, page: number, pageSize: number) => searchMock(query, page, pageSize),
    tagApply: (ids: string[], add: string[], remove: string[]) => tagApplyMock(ids, add, remove),
  },
}));

import {
  IpcError,
  frameDelete,
  libraryInfo,
  libraryOpen,
  libraryVerify,
  logClientEvent,
  search,
  settingsSet,
  tagApply,
  unwrap,
} from './ipc';

describe('shared/ipc', () => {
  beforeEach(() => {
    libraryInfoMock.mockReset();
    logClientEventMock.mockReset();
    libraryOpenMock.mockReset();
    settingsSetMock.mockReset();
    frameDeleteMock.mockReset();
    searchMock.mockReset();
    tagApplyMock.mockReset();
    libraryVerifyMock.mockReset();
  });

  it('libraryVerify returns the job id and throws when no library is open', async () => {
    libraryVerifyMock.mockResolvedValue({ status: 'ok', data: '01JOBID' });
    await expect(libraryVerify()).resolves.toBe('01JOBID');
    const error = { code: 'NotFound', message: 'No library is open.' } as const;
    libraryVerifyMock.mockResolvedValue({ status: 'error', error });
    await expect(libraryVerify()).rejects.toMatchObject({ message: error.message });
  });

  it('search passes the query and the page and returns the page of frames', async () => {
    const page = { items: [], total: 0, page: 2, page_size: 25 };
    searchMock.mockResolvedValue({ status: 'ok', data: page });
    const query = { text: 'portra', tags_all: [], date_from: '2026-07' };
    await expect(search(query, 2, 25)).resolves.toEqual(page);
    expect(searchMock).toHaveBeenCalledWith(query, 2, 25);
  });

  it('tagApply sends the names and throws the AppError of a refusal', async () => {
    tagApplyMock.mockResolvedValue({ status: 'ok', data: null });
    await expect(tagApply(['f1'], ['Night Market'], [])).resolves.toBeUndefined();
    expect(tagApplyMock).toHaveBeenCalledWith(['f1'], ['Night Market'], []);
    const error = { code: 'InvalidInput', message: 'A tag needs a name.' } as const;
    tagApplyMock.mockResolvedValue({ status: 'error', error });
    await expect(tagApply(['f1'], [''], [])).rejects.toMatchObject({ message: error.message });
  });

  it('frameDelete passes the ids and returns the report with files left', async () => {
    const report = { frames_deleted: 2, files_left: ['D:\\Lib\\originals\\r\\f\\scan.tif'] };
    frameDeleteMock.mockResolvedValue({ status: 'ok', data: report });
    await expect(frameDelete(['a', 'b'])).resolves.toEqual(report);
    expect(frameDeleteMock).toHaveBeenCalledWith(['a', 'b']);
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
