import { useCallback, useEffect, useRef, useState } from 'react';
import { clampCrop } from './crop';
import { geometryOf } from './recipe-parts';
import { useHistory } from './use-history';
import {
  IpcError,
  developAnalyze,
  developOpen,
  developRender,
  logClientEvent,
  recipeSave,
  type DevelopAnalysis,
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

/** What {@link Develop.setRecipe} takes besides the recipe. */
export interface SetRecipeOptions {
  dragging?: boolean;
  preview?: boolean;
  merge?: boolean;
}

export type DevelopStatus = 'idle' | 'opening' | 'ready' | 'error';
export type SaveStatus = 'idle' | 'saving' | 'saved' | 'error';

/** What the picture shows while something is being picked on it. */
export type PickView = 'scan' | 'base' | 'frame';

/** The frame the analysis found, as fractions of the scan, and how sure it is (0 to 1). */
export interface DetectedFrame {
  rect: { x: number; y: number; w: number; h: number };
  confidence: number;
}

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
  /** The frame the analysis found, and how sure it is (0 to 1); null until the analysis is done. */
  detected: DetectedFrame | null;
  after: Preview | null;
  before: Preview | null;
  /** The picture picking is done on: the whole scan (turned or not), or the frame as edited. */
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
  detected: null,
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
  /** The automatic recipe (what the resets go back to); null until the analysis is done. */
  auto: Recipe | null;
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
  /** The frame the analysis found and how sure it is; null until the analysis is done. */
  detected: DetectedFrame | null;
  /** The picture on screen is the whole uncropped scan as scanned, for editing the crop. */
  showingScan: boolean;
  /** The picture on screen is the whole scan turned and flipped like the frame, to pick the film base. */
  showingBase: boolean;
  /** The picture on screen is the frame as edited, for the neutral and levels pickers. */
  showingFrame: boolean;
  busy: boolean;
  renderError: string | null;
  /** Whether the recipe is being saved, was saved, or could not be. */
  saveStatus: SaveStatus;
  saveError: string | null;
  toggleBefore: () => void;
  /**
   * Shows the whole scan as scanned (`scan`), the whole scan turned and flipped (`base`) or the frame
   * as edited (`frame`) instead of the picture, or goes back to the picture with `null`.
   */
  setPickView: (view: PickView | null) => void;
  /**
   * Changes the recipe, asks for a new preview, and saves the recipe after {@link SAVE_DELAY_MS}
   * without further changes. The preview is at {@link DRAG_EDGE} (at most once per
   * {@link DRAG_DELAY_MS}) while `dragging`, at {@link RELEASE_EDGE} at once otherwise. With
   * `preview: false` no preview is asked for (the picture on screen is not the edit, as while the
   * crop is edited on the whole scan); {@link Develop.rerender} draws it afterwards.
   *
   * Every call goes through the undo history: a change with `dragging` false is one step, and a run
   * of `dragging` changes closed by the release is one step (whether or not a preview is asked for).
   * With `merge` (a slider reset) the change replaces the newest step if that is recent and changed
   * only the same fields (a double click on a slider's track is one step).
   */
  setRecipe: (recipe: Recipe, options?: SetRecipeOptions) => void;
  /** Asks for a preview of the recipe as it is now. */
  rerender: () => void;
  /** The last change is undone (the recipe goes back to what it was; it is previewed and saved). */
  undo: () => void;
  /** The last undone change is made again. */
  redo: () => void;
  canUndo: boolean;
  canRedo: boolean;
  /**
   * Reads the frame's settings again (the roll's film base may have changed) and renders it
   * afresh. The recipe being edited is kept.
   */
  refresh: () => Promise<void>;
  /**
   * Saves the edit that is waiting to be saved, now. Resolves true when nothing is left waiting
   * (it was saved, or there was nothing), false when the save failed.
   */
  flush: () => Promise<boolean>;
  /**
   * The next sequence number for a render of any frame asked for next to this one. The backend
   * drops a render older than the newest it saw for that frame, so the numbers must come from one
   * counter when two frames take turns (as when compared and swapped).
   */
  nextSeq: () => number;
}

/** The detected frame as plain numbers (a NaN travels as null, and counts as no confidence). */
function detectedOf(analysis: DevelopAnalysis): DetectedFrame {
  const rect = analysis.frame_rect;
  const confidence = analysis.frame_confidence;
  return {
    rect: clampCrop({ x: rect.x ?? 0, y: rect.y ?? 0, w: rect.w ?? 1, h: rect.h ?? 1 }),
    confidence: typeof confidence === 'number' && Number.isFinite(confidence) ? confidence : 0,
  };
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
  const { canUndo, canRedo, record, undo: undoStep, redo: redoStep, clear } = useHistory<Recipe>();

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
    // The history belongs to the frame that was open.
    clear();
    if (!frameId) {
      setState(EMPTY);
      return;
    }
    setState({ ...EMPTY, status: 'opening' });
    void (async () => {
      try {
        const info = await developOpen(frameId);
        if (gen !== generation.current) return;
        latestRecipe.current = info.recipe;
        setState((s) => ({ ...s, status: 'ready', info, recipe: info.recipe }));
        void send(frameId, info.recipe, RELEASE_EDGE, 'after', gen);
        developAnalyze(frameId, null, null).then(
          (analysis) => {
            if (gen === generation.current) {
              setState((s) => ({
                ...s,
                auto: analysis.auto_recipe,
                detected: detectedOf(analysis),
              }));
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
  }, [frameId, send, flushSave, clear]);

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
    // The whole scan as scanned (to edit the crop), the whole scan turned and flipped like the frame
    // (for the film base), or the frame as edited. A picked region is mapped back by pick-geometry.ts.
    const { rot90, flip_h, flip_v } = geometryOf(recipe);
    const geometry =
      pickView === 'scan'
        ? WHOLE_SCAN_GEOMETRY
        : pickView === 'base'
          ? { ...WHOLE_SCAN_GEOMETRY, rot90, flip_h, flip_v }
          : recipe.geometry;
    const shown = { ...recipe, geometry } as Recipe;
    void send(frame.current, shown, RELEASE_EDGE, 'pick', generation.current);
  }, [pickView, pick, status, send]);

  /** Shows, previews and saves a recipe, without telling the history. */
  const apply = useCallback(
    (recipe: Recipe, options?: { dragging?: boolean; preview?: boolean }) => {
      const id = frame.current;
      if (!id) return;
      const gen = generation.current;
      latestRecipe.current = recipe;
      setState((s) => ({
        ...s,
        recipe,
        saveStatus: s.saveStatus === 'error' ? 'idle' : s.saveStatus,
      }));
      clearTimeout(timer.current);
      if (options?.preview === false) {
        // Nothing to draw: the picture on screen is not the edit.
      } else if (options?.dragging) {
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

  const setRecipe = useCallback(
    (recipe: Recipe, options?: SetRecipeOptions) => {
      const before = latestRecipe.current;
      if (!frame.current || !before) return;
      record(before, recipe, options?.dragging === true, options?.merge === true);
      apply(recipe, options);
    },
    [record, apply],
  );

  const undo = useCallback(() => {
    const current = latestRecipe.current;
    const previous = current && undoStep(current);
    if (previous) apply(previous);
  }, [undoStep, apply]);

  const redo = useCallback(() => {
    const current = latestRecipe.current;
    const next = current && redoStep(current);
    if (next) apply(next);
  }, [redoStep, apply]);

  const rerender = useCallback(() => {
    const id = frame.current;
    const recipe = latestRecipe.current;
    if (id && recipe) void send(id, recipe, RELEASE_EDGE, 'after', generation.current);
  }, [send]);

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
      setState((s) => ({
        ...s,
        info,
        before: null,
        auto: null,
        autoFailed: false,
        detected: null,
      }));
      const recipe = latestRecipe.current ?? info.recipe;
      void send(id, recipe, RELEASE_EDGE, 'after', gen);
      developAnalyze(id, null, null).then(
        (analysis) => {
          if (gen === generation.current) {
            setState((s) => ({
              ...s,
              auto: analysis.auto_recipe,
              detected: detectedOf(analysis),
            }));
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

  const flush = useCallback(async () => {
    await flushSave(true, generation.current);
    return pendingSave.current === null;
  }, [flushSave]);

  const nextSeq = useCallback(() => {
    sequence.current += 1;
    return sequence.current;
  }, []);

  const showingPick = state.pickView !== null && state.pick !== null;
  const showingScan = showingPick && state.pickView === 'scan';
  const showingBase = showingPick && state.pickView === 'base';
  const showingFrame = showingPick && state.pickView === 'frame';
  const showingBefore = !showingPick && state.showBefore && state.before !== null;
  const preview = showingPick ? state.pick : showingBefore ? state.before : state.after;
  return {
    status: state.status,
    error: state.error,
    info: state.info,
    recipe: state.recipe,
    auto: state.auto,
    preview,
    showBefore: state.showBefore,
    showingBefore,
    beforeUnavailable: state.showBefore && state.autoFailed,
    detected: state.detected,
    showingScan,
    showingBase,
    showingFrame,
    busy: state.pending > 0,
    renderError: state.renderError,
    saveStatus: state.saveStatus,
    saveError: state.saveError,
    toggleBefore,
    setPickView,
    setRecipe,
    rerender,
    undo,
    redo,
    canUndo,
    canRedo,
    refresh,
    flush,
    nextSeq,
  };
}
