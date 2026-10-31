import { useEffect, useState } from 'react';
import { cachedThumbnail, isMissingThumbnail, loadThumbnail } from './thumbnails';

export type ThumbnailState =
  | { status: 'loading' }
  | { status: 'ready'; url: string }
  | { status: 'none' }
  | { status: 'error' };

/** The thumbnail of a frame. `expected` is false for a frame known to have none (no request). */
export function useThumbnail(id: string, expected: boolean): ThumbnailState {
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
  }, [id, expected]);

  return state;
}

function initial(id: string, expected: boolean): ThumbnailState {
  if (!expected || isMissingThumbnail(id)) return { status: 'none' };
  const url = cachedThumbnail(id);
  return url ? { status: 'ready', url } : { status: 'loading' };
}
