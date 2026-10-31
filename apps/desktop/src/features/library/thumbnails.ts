// Thumbnails reach the screen as raw JPEG bytes from the backend (`frame_thumbnail`), are turned
// into object URLs, and are kept in a small least-recently-used cache. A frame without a
// thumbnail (yet) is remembered as missing until the library changes.
import { frameThumbnail, IpcError } from '../../shared/ipc';

/** Thumbnails kept as object URLs: far more than a screen shows, so a visible one is never evicted. */
export const MAX_CACHED = 600;
/** Thumbnail requests in flight at once, so scrolling fast does not flood the backend. */
export const MAX_PARALLEL = 6;

const cache = new Map<string, string>();
const missing = new Set<string>();
const inflight = new Map<string, Promise<string | null>>();
const waiting: Array<() => void> = [];
let running = 0;

function remember(id: string, url: string) {
  cache.delete(id);
  cache.set(id, url);
  while (cache.size > MAX_CACHED) {
    const oldest = cache.entries().next().value;
    if (!oldest) break;
    cache.delete(oldest[0]);
    URL.revokeObjectURL(oldest[1]);
  }
}

async function turn(): Promise<void> {
  if (running < MAX_PARALLEL) {
    running += 1;
    return;
  }
  await new Promise<void>((resolve) => waiting.push(resolve));
}

function release() {
  const next = waiting.shift();
  if (next) next();
  else running -= 1;
}

/** The cached object URL of a thumbnail, if it is loaded. */
export function cachedThumbnail(id: string): string | null {
  const url = cache.get(id) ?? null;
  if (url) remember(id, url);
  return url;
}

export function isMissingThumbnail(id: string): boolean {
  return missing.has(id);
}

/**
 * Loads a thumbnail: its object URL, or null when the frame has none. Other failures reject.
 * Many callers asking for the same frame share one request.
 */
export function loadThumbnail(id: string): Promise<string | null> {
  const known = cachedThumbnail(id);
  if (known) return Promise.resolve(known);
  if (missing.has(id)) return Promise.resolve(null);
  const pending = inflight.get(id);
  if (pending) return pending;

  const request = (async () => {
    await turn();
    try {
      const bytes = await frameThumbnail(id);
      const url = URL.createObjectURL(new Blob([bytes], { type: 'image/jpeg' }));
      remember(id, url);
      return url;
    } catch (error) {
      if (error instanceof IpcError && error.error.code === 'NotFound') {
        missing.add(id);
        return null;
      }
      throw error;
    } finally {
      release();
      inflight.delete(id);
    }
  })();
  inflight.set(id, request);
  return request;
}

/** Forgets that thumbnails were missing (call when the library changes: they may exist now). */
export function forgetMissingThumbnails() {
  missing.clear();
}

/** Drops one thumbnail, for example after it was regenerated. */
export function invalidateThumbnail(id: string) {
  const url = cache.get(id);
  if (url) URL.revokeObjectURL(url);
  cache.delete(id);
  missing.delete(id);
}

/** Drops everything (another library was opened). */
export function resetThumbnails() {
  for (const url of cache.values()) URL.revokeObjectURL(url);
  cache.clear();
  missing.clear();
}
