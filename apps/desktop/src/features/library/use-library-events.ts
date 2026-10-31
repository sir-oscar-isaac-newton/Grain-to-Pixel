import { useEffect } from 'react';
import { onJobDone, onJobError, onJobProgress, onLibraryChanged } from '../../shared/ipc';
import { useLibraryStore } from './library-store';
import { forgetMissingThumbnails } from './thumbnails';

/** Changes from an import arrive one frame at a time; reload once after this quiet time. */
export const RELOAD_DELAY_MS = 150;

/**
 * Keeps the screen in step with the backend: reloads the frames and rolls when the library
 * changes, and follows the progress of the jobs started from this screen.
 */
export function useLibraryEvents(enabled: boolean) {
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const stops: Array<() => void> = [];
    const track = (listening: Promise<() => void>) => {
      listening.then(
        (stop) => (alive ? stops.push(stop) : stop()),
        () => {
          // No backend (tests, plain browser): there is nothing to listen to.
        },
      );
    };
    const store = useLibraryStore.getState;

    track(
      onLibraryChanged(() => {
        clearTimeout(timer);
        timer = setTimeout(() => {
          forgetMissingThumbnails();
          store().bump();
          // The rolls change too (an import can set a roll's film base).
          store()
            .reloadRolls()
            .catch(() => {
              // The next change or a restart reloads them; the grid is already up to date.
            });
        }, RELOAD_DELAY_MS);
      }),
    );
    track(onJobProgress((e) => store().patchJob(e.id, { fraction: e.fraction })));
    track(
      onJobDone((e) => {
        const result = e.result;
        store().patchJob(
          e.id,
          result.kind === 'Imported'
            ? {
                state: 'done',
                fraction: 1,
                imported: result.frames.length,
                skipped: result.skipped,
              }
            : { state: 'done', fraction: 1 },
        );
      }),
    );
    track(
      onJobError((e) =>
        store().patchJob(
          e.id,
          e.error.code === 'Cancelled'
            ? { state: 'cancelled' }
            : { state: 'error', message: e.error.message },
        ),
      ),
    );

    return () => {
      alive = false;
      clearTimeout(timer);
      stops.forEach((stop) => stop());
    };
  }, [enabled]);
}
