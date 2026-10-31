import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();
const openMock = vi.fn();
const folderCheckMock = vi.fn();
const jobCancelMock = vi.fn();
const developOpenMock = vi.fn();
const developAnalyzeMock = vi.fn();
const developSampleBaseMock = vi.fn();
const developSampleLevelsMock = vi.fn();
const recipeSaveMock = vi.fn();
const recipeApplyToMock = vi.fn();
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
      developOpen: (id: string) => developOpenMock(id),
      developAnalyze: (id: string, base: unknown) => developAnalyzeMock(id, base),
      developSampleBase: (id: string, region: unknown) => developSampleBaseMock(id, region),
      developSampleLevels: (...args: unknown[]) => developSampleLevelsMock(...args),
      recipeSave: (...args: unknown[]) => recipeSaveMock(...args),
      recipeApplyTo: (...args: unknown[]) => recipeApplyToMock(...args),
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
  developAnalyze,
  developOpen,
  developSampleBase,
  developSampleLevels,
  developRender,
  folderCheck,
  frameThumbnail,
  jobCancel,
  onJobDone,
  onJobError,
  onJobProgress,
  onLibraryChanged,
  parsePreview,
  pickFolder,
  pickTiffFiles,
  recipeApplyTo,
  recipeSave,
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

  describe('develop', () => {
    /** The bytes of `develop_render`: seq u64, width u32, height u32 (little-endian), then RGBA8. */
    function previewBytes(seq: number, width: number, height: number, pixels?: number[]) {
      const rgba = pixels ?? new Array<number>(width * height * 4).fill(7);
      const buffer = new ArrayBuffer(16 + rgba.length);
      const view = new DataView(buffer);
      view.setBigUint64(0, BigInt(seq), true);
      view.setUint32(8, width, true);
      view.setUint32(12, height, true);
      new Uint8Array(buffer, 16).set(rgba);
      return buffer;
    }

    beforeEach(() => {
      developOpenMock.mockReset();
      developAnalyzeMock.mockReset();
      developSampleBaseMock.mockReset();
      developSampleLevelsMock.mockReset();
      recipeSaveMock.mockReset();
      recipeApplyToMock.mockReset();
    });

    it('reads the header and the pixels of a preview', () => {
      const pixels = [1, 2, 3, 255, 4, 5, 6, 255];
      const preview = parsePreview(previewBytes(300, 2, 1, pixels));
      expect(preview).toMatchObject({ seq: 300, width: 2, height: 1 });
      expect([...preview.rgba]).toEqual(pixels);
      expect(preview.rgba).toBeInstanceOf(Uint8ClampedArray);
    });

    it('refuses bytes that are too short or do not match their size', () => {
      expect(() => parsePreview(new ArrayBuffer(8))).toThrow('incomplete');
      expect(() => parsePreview(previewBytes(1, 2, 2).slice(0, 20))).toThrow('does not match');
      expect(() => parsePreview(previewBytes(1, 2, 2, [1, 2, 3, 4]))).toThrow('does not match');
    });

    it('renders through the raw command with camel-case arguments', async () => {
      invokeMock.mockResolvedValue(previewBytes(5, 1, 1, [9, 8, 7, 255]));
      const recipe = { v: 1 } as unknown as Parameters<typeof developRender>[1];
      const preview = await developRender('f1', recipe, 1600, 5);
      expect(invokeMock).toHaveBeenCalledWith('develop_render', {
        frameId: 'f1',
        recipe,
        maxEdge: 1600,
        seq: 5,
      });
      expect(preview.seq).toBe(5);
      expect([...preview.rgba]).toEqual([9, 8, 7, 255]);
    });

    it('turns a dropped render into an IpcError with the Cancelled code', async () => {
      const error = { code: 'Cancelled', message: 'A newer preview was requested.' };
      invokeMock.mockRejectedValueOnce(error);
      const failure = developRender('f1', {} as never, 1600, 1);
      await expect(failure).rejects.toBeInstanceOf(IpcError);
      await expect(failure).rejects.toMatchObject({ error });
    });

    it('saves a recipe and returns what was stored, or throws what the backend refuses', async () => {
      const recipe = { v: 1 } as unknown as Parameters<typeof recipeSave>[1];
      const stored = { v: 1, tone: { exposure_ev: 5 } };
      recipeSaveMock.mockResolvedValueOnce({ status: 'ok', data: stored });
      await expect(recipeSave('f1', recipe)).resolves.toBe(stored);
      expect(recipeSaveMock).toHaveBeenCalledWith('f1', recipe);

      recipeSaveMock.mockResolvedValueOnce({
        status: 'error',
        error: { code: 'InvalidInput', message: 'A curve needs at least two points.' },
      });
      await expect(recipeSave('f1', recipe)).rejects.toBeInstanceOf(IpcError);
    });

    it('applies chosen groups of settings to frames', async () => {
      recipeApplyToMock.mockResolvedValueOnce({ status: 'ok', data: null });
      await expect(recipeApplyTo(['f2', 'f3'], 'f1', ['Tone', 'Color'])).resolves.toBeUndefined();
      expect(recipeApplyToMock).toHaveBeenCalledWith(['f2', 'f3'], 'f1', ['Tone', 'Color']);

      recipeApplyToMock.mockResolvedValueOnce({
        status: 'error',
        error: { code: 'NotFound', message: 'No such frame.' },
      });
      await expect(recipeApplyTo(['x'], 'f1', ['Tone'])).rejects.toBeInstanceOf(IpcError);
    });

    it('samples the base from an area of the scan, in fractions of it', async () => {
      const region = { x: 0.1, y: 0.2, w: 0.05, h: 0.4 };
      developSampleBaseMock.mockResolvedValueOnce({ status: 'ok', data: [0.8, 0.5, 0.3] });
      await expect(developSampleBase('f1', region)).resolves.toEqual([0.8, 0.5, 0.3]);
      expect(developSampleBaseMock).toHaveBeenCalledWith('f1', region);

      developSampleBaseMock.mockResolvedValueOnce({
        status: 'error',
        error: { code: 'InvalidInput', message: 'That area is too dark to be film base.' },
      });
      await expect(developSampleBase('f1', region)).rejects.toBeInstanceOf(IpcError);
    });

    it('samples a white or black point from an area of the frame', async () => {
      const recipe = { v: 1 } as unknown as Parameters<typeof developSampleLevels>[1];
      const region = { x: 0.9, y: 0.9, w: 0.05, h: 0.05 };
      const levels = { black: [0.1, 0.1, 0.1], white: [2, 2.1, 2.2] };
      developSampleLevelsMock.mockResolvedValueOnce({ status: 'ok', data: levels });
      await expect(developSampleLevels('f1', recipe, region, 'white')).resolves.toBe(levels);
      expect(developSampleLevelsMock).toHaveBeenCalledWith('f1', recipe, region, 'white');

      developSampleLevelsMock.mockResolvedValueOnce({
        status: 'error',
        error: { code: 'InvalidInput', message: 'That area is black.' },
      });
      await expect(developSampleLevels('f1', recipe, region, 'black')).rejects.toBeInstanceOf(
        IpcError,
      );
    });

    it('opens and analyses through the typed commands', async () => {
      developOpenMock.mockResolvedValue({ status: 'ok', data: { width: 6000 } });
      await expect(developOpen('f1')).resolves.toEqual({ width: 6000 });
      expect(developOpenMock).toHaveBeenCalledWith('f1');

      developAnalyzeMock.mockResolvedValue({
        status: 'error',
        error: { code: 'NotFound', message: 'This frame has no scan file.' },
      });
      await expect(developAnalyze('f1', null)).rejects.toBeInstanceOf(IpcError);
      expect(developAnalyzeMock).toHaveBeenCalledWith('f1', null);

      // A base picked on the rebate is passed on, so the levels are fitted to it.
      developAnalyzeMock.mockResolvedValue({ status: 'ok', data: { base_given: true } });
      await expect(developAnalyze('f1', [0.8, 0.5, 0.3])).resolves.toEqual({ base_given: true });
      expect(developAnalyzeMock).toHaveBeenLastCalledWith('f1', [0.8, 0.5, 0.3]);
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
      const seen: Array<[string | null, string[]]> = [];
      const unlisten = await onLibraryChanged((roll, frames) => seen.push([roll, frames]));
      handlers.libraryChanged?.({ payload: { roll_id: 'r1', frame_ids: [] } });
      handlers.libraryChanged?.({ payload: { roll_id: null, frame_ids: ['f1', 'f2'] } });
      handlers.libraryChanged?.({ payload: {} });
      expect(seen).toEqual([
        ['r1', []],
        [null, ['f1', 'f2']],
        [null, []],
      ]);
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
