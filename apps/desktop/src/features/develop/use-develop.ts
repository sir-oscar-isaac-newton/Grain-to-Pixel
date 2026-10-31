import { useCallback, useEffect, useRef, useState } from 'react';
import {
  IpcError,
  developAnalyze,
  developOpen,
  developRender,
  logClientEvent,
  recipeSave,
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
/** A changed recipe is saved once it has been left alone for this long. */
export const SAVE_DELAY_MS = 500;

/** The geometry that shows the whole scan: no crop, no turn, no straightening (for picking). */
export const WHOLE_SCAN_GEOMETRY = {
  crop: { x: 0, y: 0, w: 1, h: 1 },
  angle_deg: 0,
  rot90: 0,
  flip_h: false,
  flip_v: false,
};

export type DevelopStatus = 'idle' | 'opening' | 'ready' | 'error';
export type SaveStatus = 'idle' | 'saving' | 'saved' | 'error';

/** What the picture shows while something is being picked on it. */
export type PickView = 'scan' | 'frame';

type Slot = 'after' | 'before' | 'pick';

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
  /** The picture picking is done on: the whole scan, or the frame without turns. */
  pick: Preview | null;
  /** The backend's message when a render failed; the last picture stays. */
  renderError: string | null;
  showBefore: boolean;
  pickView: PickView | null;
  saveStatus: SaveStatus;
  saveError: string | null;
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
  pick: null,
  renderError: null,
  showBefore: false,
  pickView: null,
  saveStatus: 'idle',
  saveError: null,
  pending: 0,
};

export interface Develop {
  status: DevelopStatus;
  error: string | null;
  info: DevelopInfo | null;
  recipe: Recipe | null;
  /**
   * The picture to show now: the picking view while picking (once it has arrived), else the "before"
   * one when it is on and has arrived, else the edit.
   */
  preview: Preview | null;
  showBefore: boolean;
  /** "Before" is on and its picture is on screen. */
  showingBefore: boolean;
  /** "Before" cannot be shown: the automatic recipe is not known (the analysis failed). */
  beforeUnavailable: boolean;
  /** The picture on screen is the whole uncropped scan, so a click on it can pick on the rebate. */
  showingScan: boolean;
  /** The picture on screen is the frame without turns or straightening, for the levels pickers. */
  showingFrame: boolean;
  busy: boolean;
  renderError: string | null;
  /** Whether the recipe is being saved, was saved, or could not be. */
  saveStatus: SaveStatus;
  saveError: string | null;
  toggleBefore: () => void;
  /**
   * Shows the whole scan (no crop or turn) or the frame without turns or straightening instead of
   * the picture, or goes back to the picture with 
ull.
   */
  setPickView: (view: PickView | null) => void;
  /**
   * Changes the recipe, asks for a new preview, and saves the recipe after {@link SAVE_DELAY_MS}
   * without further changes. The preview is at {@link DRAG_EDGE} (at most once per
   * {@link DRAG_DELAY_MS}) while `dragging`, at {@link RELEASE_EDGE} at once otherwise.
   */
  setRecipe: (recipe: Recipe, options?: { dragging?: boolean }) => void;
  /**
   * Reads the frame's settings again (the roll's film base may have changed) and renders it
   * afresh. The recipe being edited is kept.
   */
  refresh: () => Promise<void>;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Opens a frame for editing and keeps its preview up to date. Every render request carries a
 * sequence number; a picture older than the newest one shown is dropped, and so is one that
 * belongs to a frame that is no longer open. A request the backend superseded (`Cancelled`) is not
 * a failure. Edits are saved without being asked, and a pending save is made when another frame
 * is opened or the view closes.
 */
export function useDevelop(frameId: string | null): Develop {
  const [state, setState] = useState<State>(EMPTY);
  const frame = useRef<string | null>(null);
  const generation = useRef(0);
  const sequence = useRef(0);
  const accepted = useRef<Record<Slot, number>>({ after: -1, before: -1, pick: -1 });
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const pendingSave = useRef<{ id: string; recipe: Recipe } | null>(null);
  const latestRecipe = useRef<Recipe | null>(null);
  latestRecipe.current = state.recipe;

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

  /** Saves the recipe waiting to be saved, if there is one. `report` says whether to show it. */
  const flushSave = useCallback(async (report: boolean, gen: number) => {
    clearTimeout(saveTimer.current);
    const job = pendingSave.current;
    if (!job) return;
    pendingSave.current = null;
    if (report && gen === generation.current) {
      setState((s) => ({ ...s, saveStatus: 'saving', saveError: null }));
    }
    try {
      await recipeSave(job.id, job.recipe);
      if (report && gen === generation.current && !pendingSave.current) {
        setState((s) => ({ ...s, saveStatus: 'saved' }));
      }
    } catch (error) {
      if (report && gen === generation.current) {
        // Kept, so the next change (or the next flush) tries again with the newest recipe.
        pendingSave.current ??= job;
        setState((s) => ({ ...s, saveStatus: 'error', saveError: messageOf(error) }));
      } else if (!report) {
        // Leaving the frame: there is no screen to tell, so the log keeps the record.
        void logClientEvent(
          'Warn',
          `An edit could not be saved when its frame was closed: ${messageOf(error)}`,
        );
      }
    }
  }, []);

  // Opens the frame, shows it, and finds the automatic recipe in the background.
  useEffect(() => {
    generation.current += 1;
    const gen = generation.current;
    clearTimeout(timer.current);
    accepted.current = { after: -1, before: -1, pick: -1 };
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
        developAnalyze(frameId, null).then(
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
      // A new frame, or the view closing: nothing from this frame is wanted any more, except the
      // edit that was waiting to be saved.
      void flushSave(false, gen);
      generation.current += 1;
      clearTimeout(timer.current);
    };
  }, [frameId, send, flushSave]);

  // "Before" is rendered the first time it is wanted, from the automatic recipe.
  const { showBefore, auto, before, status, pickView, pick } = state;
  useEffect(() => {
    if (!showBefore || !auto || before || status !== 'ready' || !frame.current) return;
    void send(frame.current, auto, RELEASE_EDGE, 'before', generation.current);
  }, [showBefore, auto, before, status, send]);

  // The picking view is rendered when picking starts, from the recipe as it is then.
  useEffect(() => {
    const recipe = latestRecipe.current;
    if (!pickView || pick || status !== 'ready' || !frame.current || !recipe) return;
    const geometry =
      pickView === 'scan'
        ? WHOLE_SCAN_GEOMETRY
        : { ...recipe.geometry, angle_deg: 0, rot90: 0, flip_h: false, flip_v: false };
    const shown = { ...recipe, geometry } as Recipe;
    void send(frame.current, shown, RELEASE_EDGE, 'pick', generation.current);
  }, [pickView, pick, status, send]);

  const setRecipe = useCallback(
    (recipe: Recipe, options?: { dragging?: boolean }) => {
      const id = frame.current;
      if (!id) return;
      const gen = generation.current;
      setState((s) => ({
        ...s,
        recipe,
        saveStatus: s.saveStatus === 'error' ? 'idle' : s.saveStatus,
      }));
      clearTimeout(timer.current);
      if (options?.dragging) {
        timer.current = setTimeout(() => {
          void send(id, recipe, DRAG_EDGE, 'after', gen);
        }, DRAG_DELAY_MS);
      } else {
        void send(id, recipe, RELEASE_EDGE, 'after', gen);
      }
      pendingSave.current = { id, recipe };
      clearTimeout(saveTimer.current);
      saveTimer.current = setTimeout(() => {
        void flushSave(true, gen);
      }, SAVE_DELAY_MS);
    },
    [send, flushSave],
  );

  const toggleBefore = useCallback(() => {
    setState((s) => ({ ...s, showBefore: !s.showBefore }));
  }, []);

  const setPickView = useCallback((view: PickView | null) => {
    // The picking picture is dropped when it goes, so the next time it shows the recipe as it is then.
    accepted.current.pick = -1;
    setState((s) => ({ ...s, pickView: view, pick: null }));
  }, []);

  const refresh = useCallback(async () => {
    const id = frame.current;
    if (!id) return;
    const gen = generation.current;
    try {
      const info = await developOpen(id);
      if (gen !== generation.current) return;
      // "Before" depends on the roll's base too: forget it, and ask the analysis again.
      accepted.current.before = -1;
      setState((s) => ({ ...s, info, before: null, auto: null, autoFailed: false }));
      const recipe = latestRecipe.current ?? info.recipe;
      void send(id, recipe, RELEASE_EDGE, 'after', gen);
      developAnalyze(id, null).then(
        (analysis) => {
          if (gen === generation.current) {
            setState((s) => ({ ...s, auto: analysis.auto_recipe }));
          }
        },
        () => {
          if (gen === generation.current) setState((s) => ({ ...s, autoFailed: true }));
        },
      );
    } catch (error) {
      if (gen === generation.current) {
        setState((s) => ({ ...s, renderError: messageOf(error) }));
      }
    }
  }, [send]);

  const showingPick = state.pickView !== null && state.pick !== null;
  const showingScan = showingPick && state.pickView === 'scan';
  const showingFrame = showingPick && state.pickView === 'frame';
  const showingBefore = !showingPick && state.showBefore && state.before !== null;
  const preview = showingPick ? state.pick : showingBefore ? state.before : state.after;
  return {
    status: state.status,
    error: state.error,
    info: state.info,
    recipe: state.recipe,
    preview,
    showBefore: state.showBefore,
    showingBefore,
    beforeUnavailable: state.showBefore && state.autoFailed,
    showingScan,
    showingFrame,
    busy: state.pending > 0,
    renderError: state.renderError,
    saveStatus: state.saveStatus,
    saveError: state.saveError,
    toggleBefore,
    setPickView,
    setRecipe,
    refresh,
  };
}
