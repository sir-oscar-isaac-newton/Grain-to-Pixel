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
const versions = new Map<string, number>();
const listeners = new Set<() => void>();
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
    for (;;) {
      const version = thumbnailVersion(id);
      await turn();
      try {
        const bytes = await frameThumbnail(id);
        // Regenerated while this was on its way: what arrived may be the old picture, ask again.
        if (thumbnailVersion(id) !== version) continue;
        const url = URL.createObjectURL(new Blob([bytes], { type: 'image/jpeg' }));
        remember(id, url);
        return url;
      } catch (error) {
        if (thumbnailVersion(id) !== version) continue;
        if (error instanceof IpcError && error.error.code === 'NotFound') {
          missing.add(id);
          return null;
        }
        throw error;
      } finally {
        release();
      }
    }
  })();
  inflight.set(id, request);
  const done = () => {
    if (inflight.get(id) === request) inflight.delete(id);
  };
  request.then(done, done);
  return request;
}

/** Forgets that thumbnails were missing (call when the library changes: they may exist now). */
export function forgetMissingThumbnails() {
  missing.clear();
}

/**
 * Drops one thumbnail after it was regenerated, and tells the cells showing it to load it again
 * (see `thumbnailVersion`). A request already in flight notices the change and asks again.
 */
export function invalidateThumbnail(id: string) {
  const url = cache.get(id);
  if (url) URL.revokeObjectURL(url);
  cache.delete(id);
  missing.delete(id);
  versions.set(id, thumbnailVersion(id) + 1);
  for (const listener of [...listeners]) listener();
}

/** How many times a frame's thumbnail was invalidated; a cell reloads its picture when it changes. */
export function thumbnailVersion(id: string): number {
  return versions.get(id) ?? 0;
}

/** Calls `listener` whenever any thumbnail is invalidated. Returns a function that stops. */
export function subscribeThumbnails(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Drops everything (another library was opened). */
export function resetThumbnails() {
  for (const url of cache.values()) URL.revokeObjectURL(url);
  cache.clear();
  missing.clear();
  versions.clear();
}
