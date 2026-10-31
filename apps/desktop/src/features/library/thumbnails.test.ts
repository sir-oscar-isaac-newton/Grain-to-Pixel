import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ipc, resetFake, state } from './fake-ipc';
import {
  MAX_CACHED,
  MAX_PARALLEL,
  cachedThumbnail,
  forgetMissingThumbnails,
  invalidateThumbnail,
  isMissingThumbnail,
  loadThumbnail,
  resetThumbnails,
} from './thumbnails';

vi.mock('../../shared/ipc', async () => (await import('./fake-ipc')).ipc);

describe('thumbnail cache', () => {
  beforeEach(() => {
    resetFake();
    resetThumbnails();
    vi.spyOn(URL, 'createObjectURL').mockImplementation(() => `blob:${Math.random()}`);
    vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => undefined);
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('turns the bytes into an object URL and serves the next request from the cache', async () => {
    const url = await loadThumbnail('a');
    expect(url).toMatch(/^blob:/);
    expect(await loadThumbnail('a')).toBe(url);
    expect(cachedThumbnail('a')).toBe(url);
    expect(ipc.frameThumbnail).toHaveBeenCalledTimes(1);
    const blob = vi.mocked(URL.createObjectURL).mock.calls[0]?.[0] as Blob;
    expect(blob.type).toBe('image/jpeg');
    expect(blob.size).toBe(5);
  });

  it('shares one request between callers asking for the same frame', async () => {
    const [x, y] = await Promise.all([loadThumbnail('b'), loadThumbnail('b')]);
    expect(x).toBe(y);
    expect(ipc.frameThumbnail).toHaveBeenCalledTimes(1);
  });

  it('remembers a frame without a thumbnail until told to forget, and rethrows other errors', async () => {
    state.noThumb.add('gone');
    expect(await loadThumbnail('gone')).toBeNull();
    expect(isMissingThumbnail('gone')).toBe(true);
    expect(await loadThumbnail('gone')).toBeNull();
    expect(ipc.frameThumbnail).toHaveBeenCalledTimes(1);

    state.noThumb.delete('gone');
    forgetMissingThumbnails();
    expect(isMissingThumbnail('gone')).toBe(false);
    expect(await loadThumbnail('gone')).toMatch(/^blob:/);

    ipc.frameThumbnail.mockRejectedValueOnce(new Error('disk on fire'));
    await expect(loadThumbnail('broken')).rejects.toThrow('disk on fire');
    expect(isMissingThumbnail('broken')).toBe(false);
  });

  it('asks the backend for a limited number of thumbnails at a time', async () => {
    let active = 0;
    let most = 0;
    ipc.frameThumbnail.mockImplementation(async () => {
      active += 1;
      most = Math.max(most, active);
      await new Promise((resolve) => setTimeout(resolve, 5));
      active -= 1;
      return new ArrayBuffer(4);
    });
    await Promise.all(Array.from({ length: MAX_PARALLEL * 3 }, (_, i) => loadThumbnail(`p${i}`)));
    expect(most).toBe(MAX_PARALLEL);
    expect(ipc.frameThumbnail).toHaveBeenCalledTimes(MAX_PARALLEL * 3);
  });

  it('keeps the most recently used thumbnails and releases the oldest object URLs', async () => {
    for (let i = 0; i < MAX_CACHED; i += 1) await loadThumbnail(`t${i}`);
    expect(cachedThumbnail('t0')).not.toBeNull(); // touched: now the newest
    await loadThumbnail('extra');
    expect(URL.revokeObjectURL).toHaveBeenCalledTimes(1);
    expect(cachedThumbnail('t1')).toBeNull(); // the oldest went
    expect(cachedThumbnail('t0')).not.toBeNull();
    expect(cachedThumbnail('extra')).not.toBeNull();
  });

  it('can drop one thumbnail or all of them', async () => {
    await loadThumbnail('one');
    await loadThumbnail('two');
    invalidateThumbnail('one');
    expect(cachedThumbnail('one')).toBeNull();
    expect(URL.revokeObjectURL).toHaveBeenCalledTimes(1);
    resetThumbnails();
    expect(cachedThumbnail('two')).toBeNull();
    expect(URL.revokeObjectURL).toHaveBeenCalledTimes(2);
  });
});
