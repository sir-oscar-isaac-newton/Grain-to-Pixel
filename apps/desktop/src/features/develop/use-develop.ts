import { useCallback, useEffect, useRef, useState } from 'react';
import {
  IpcError,
  developAnalyze,
  developOpen,
  developRender,
  type DevelopInfo,
  type Preview,
  type Recipe,
} from '../../shared/ipc';

/** Long edge of a preview asked for while a slider is being dragged. */
export const DRAG_EDGE = 1600;
/** Long edge of a preview asked for when the frame opens and when a drag is released. */
export const RELEASE_EDGE = 2048;
/** A drag asks for a new preview at most this often. */
export const DRAG_DELAY_MS = 40;

export type DevelopStatus = 'idle' | 'opening' | 'ready' | 'error';

type Slot = 'after' | 'before';

interface State {
  status: DevelopStatus;
  /** Why the frame could not be opened. */
  error: string | null;
  info: DevelopInfo | null;
  /** The recipe being edited (starts as the saved one). */
  recipe: Recipe | null;
  /** The automatic recipe (the reference for "before"); null until the analysis is done. */
  auto: Recipe | null;
  /** The analysis failed, so "before" cannot be shown. */
  autoFailed: boolean;
  after: Preview | null;
  before: Preview | null;
  /** The backend's message when a render failed; the last picture stays. */
  renderError: string | null;
  showBefore: boolean;
  /** Renders asked for and not answered yet. */
  pending: number;
}

const EMPTY: State = {
  status: 'idle',
  error: null,
  info: null,
  recipe: null,
  auto: null,
  autoFailed: false,
  after: null,
  before: null,
  renderError: null,
  showBefore: false,
  pending: 0,
};

export interface Develop {
  status: DevelopStatus;
  error: string | null;
  info: DevelopInfo | null;
  recipe: Recipe | null;
  /** The picture to show now: the "before" one when it is on and has arrived, else the edit. */
  preview: Preview | null;
  showBefore: boolean;
  /** "Before" is on and its picture is on screen. */
  showingBefore: boolean;
  /** "Before" cannot be shown: the automatic recipe is not known (the analysis failed). */
  beforeUnavailable: boolean;
  busy: boolean;
  renderError: string | null;
  toggleBefore: () => void;
  /**
   * Changes the recipe and asks for a new preview: at {@link DRAG_EDGE} (at most once per
   * {@link DRAG_DELAY_MS}) while `dragging`, at {@link RELEASE_EDGE} at once otherwise.
   */
  setRecipe: (recipe: Recipe, options?: { dragging?: boolean }) => void;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Opens a frame for editing and keeps its preview up to date. Every render request carries a
 * sequence number; a picture older than the newest one shown is dropped, and so is one that
 * belongs to a frame that is no longer open. A request the backend superseded (`Cancelled`) is not
 * a failure.
 */
export function useDevelop(frameId: string | null): Develop {
  const [state, setState] = useState<State>(EMPTY);
  const frame = useRef<string | null>(null);
  const generation = useRef(0);
  const sequence = useRef(0);
  const accepted = useRef<Record<Slot, number>>({ after: -1, before: -1 });
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const send = useCallback(
    async (id: string, recipe: Recipe, edge: number, slot: Slot, gen: number) => {
      sequence.current += 1;
      const seq = sequence.current;
      setState((s) => ({ ...s, pending: s.pending + 1 }));
      let picture: Preview | null = null;
      let failure: string | null = null;
      try {
        picture = await developRender(id, recipe, edge, seq);
      } catch (error) {
        if (!(error instanceof IpcError && error.error.code === 'Cancelled')) {
          failure = messageOf(error);
        }
      }
      if (gen !== generation.current) return;
      // Decided here, once, and not inside the updater below: React may run an updater more than
      // once, and the second run would find the sequence number already taken.
      let fresh: Preview | null = null;
      if (picture && picture.seq > accepted.current[slot]) {
        accepted.current[slot] = picture.seq;
        fresh = picture;
      }
      setState((s) => {
        const next = { ...s, pending: s.pending - 1 };
        if (failure) next.renderError = failure;
        if (fresh) {
          next[slot] = fresh;
          next.renderError = null;
        }
        return next;
      });
    },
    [],
  );

  // Opens the frame, shows it, and finds the automatic recipe in the background.
  useEffect(() => {
    generation.current += 1;
    const gen = generation.current;
    clearTimeout(timer.current);
    accepted.current = { after: -1, before: -1 };
    frame.current = frameId;
    if (!frameId) {
      setState(EMPTY);
      return;
    }
    setState({ ...EMPTY, status: 'opening' });
    void (async () => {
      try {
        const info = await developOpen(frameId);
        if (gen !== generation.current) return;
        setState((s) => ({ ...s, status: 'ready', info, recipe: info.recipe }));
        void send(frameId, info.recipe, RELEASE_EDGE, 'after', gen);
        developAnalyze(frameId).then(
          (analysis) => {
            if (gen === generation.current) {
              setState((s) => ({ ...s, auto: analysis.auto_recipe }));
            }
          },
          () => {
            // The edit itself is not affected; only "before" cannot be shown.
            if (gen === generation.current) setState((s) => ({ ...s, autoFailed: true }));
          },
        );
      } catch (error) {
        if (gen === generation.current) {
          setState((s) => ({ ...s, status: 'error', error: messageOf(error) }));
        }
      }
    })();
    return () => {
      // A new frame, or the view closing: nothing from this frame is wanted any more.
      generation.current += 1;
      clearTimeout(timer.current);
    };
  }, [frameId, send]);

  // "Before" is rendered the first time it is wanted, from the automatic recipe.
  const { showBefore, auto, before, status } = state;
  useEffect(() => {
    if (!showBefore || !auto || before || status !== 'ready' || !frame.current) return;
    void send(frame.current, auto, RELEASE_EDGE, 'before', generation.current);
  }, [showBefore, auto, before, status, send]);

  const setRecipe = useCallback(
    (recipe: Recipe, options?: { dragging?: boolean }) => {
      const id = frame.current;
      if (!id) return;
      const gen = generation.current;
      setState((s) => ({ ...s, recipe }));
      clearTimeout(timer.current);
      if (options?.dragging) {
        timer.current = setTimeout(() => {
          void send(id, recipe, DRAG_EDGE, 'after', gen);
        }, DRAG_DELAY_MS);
      } else {
        void send(id, recipe, RELEASE_EDGE, 'after', gen);
      }
    },
    [send],
  );

  const toggleBefore = useCallback(() => {
    setState((s) => ({ ...s, showBefore: !s.showBefore }));
  }, []);

  const showingBefore = state.showBefore && state.before !== null;
  return {
    status: state.status,
    error: state.error,
    info: state.info,
    recipe: state.recipe,
    preview: showingBefore ? state.before : state.after,
    showBefore: state.showBefore,
    showingBefore,
    beforeUnavailable: state.showBefore && state.autoFailed,
    busy: state.pending > 0,
    renderError: state.renderError,
    toggleBefore,
    setRecipe,
  };
}
