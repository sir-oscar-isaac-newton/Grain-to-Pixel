import { useEffect, useState, useSyncExternalStore } from 'react';
import {
  cachedThumbnail,
  isMissingThumbnail,
  loadThumbnail,
  subscribeThumbnails,
  thumbnailVersion,
} from './thumbnails';

export type ThumbnailState =
  | { status: 'loading' }
  | { status: 'ready'; url: string }
  | { status: 'none' }
  | { status: 'error' };

/**
 * The thumbnail of a frame. `expected` is false for a frame known to have none (no request). It
 * loads again when the frame's thumbnail was regenerated (its version changed).
 */
export function useThumbnail(id: string, expected: boolean): ThumbnailState {
  const version = useSyncExternalStore(subscribeThumbnails, () => thumbnailVersion(id));
  const [state, setState] = useState<ThumbnailState>(() => initial(id, expected));

  useEffect(() => {
    let alive = true;
    setState(initial(id, expected));
    if (expected && !cachedThumbnail(id) && !isMissingThumbnail(id)) {
      loadThumbnail(id).then(
        (url) => {
          if (alive) setState(url ? { status: 'ready', url } : { status: 'none' });
        },
        () => {
          if (alive) setState({ status: 'error' });
        },
      );
    }
    return () => {
      alive = false;
    };
  }, [id, expected, version]);

  return state;
}

function initial(id: string, expected: boolean): ThumbnailState {
  if (!expected || isMissingThumbnail(id)) return { status: 'none' };
  const url = cachedThumbnail(id);
  return url ? { status: 'ready', url } : { status: 'loading' };
}
