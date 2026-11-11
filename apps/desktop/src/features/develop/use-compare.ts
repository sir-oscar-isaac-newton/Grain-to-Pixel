import { useCallback, useEffect, useRef, useState } from 'react';
import { IpcError, developOpen, developRender, type Preview, type Recipe } from '../../shared/ipc';
import { RELEASE_EDGE } from './use-develop';
import { PROXY_EDGE } from './viewport';

/** The frame compared with: its id, and its place in the filmstrip (for opening it on a swap). */
export interface Reference {
  id: string;
  index: number;
}

export interface Compare {
  /** Compare mode is on: with no reference yet, a filmstrip click chooses one. */
  armed: boolean;
  /** The frame compared with; never the current frame. */
  reference: Reference | null;
  /** Turns compare mode on, or off (leaving it). */
  toggle: () => void;
  /** Makes a frame the reference (turns compare mode on). */
  choose: (reference: Reference) => void;
  /** Closes the reference and leaves compare mode. */
  leave: () => void;
}

/**
 * Compare mode of the Develop view: whether it is on and which frame is the reference. When the
 * current frame becomes the reference (a click or a key on the filmstrip), the reference is
 * dropped and compare mode stays on, asking for another one.
 */
export function useCompare(currentId: string): Compare {
  const [armed, setArmed] = useState(false);
  const [chosen, setChosen] = useState<Reference | null>(null);

  useEffect(() => {
    if (chosen && chosen.id === currentId) setChosen(null);
  }, [chosen, currentId]);

  const toggle = useCallback(() => {
    setArmed((on) => !on);
    setChosen(null);
  }, []);
  const choose = useCallback((reference: Reference) => {
    setArmed(true);
    setChosen(reference);
  }, []);
  const leave = useCallback(() => {
    setArmed(false);
    setChosen(null);
  }, []);

  return {
    armed,
    reference: chosen && chosen.id !== currentId ? chosen : null,
    toggle,
    choose,
    leave,
  };
}

/** Smallest and largest long edge of the reference's picture, and the steps it is asked in. */
const MIN_EDGE = 256;
const EDGE_STEP = 128;

/** The long edge to ask for so the picture fills a pane of this size (null: not measured yet). */
export function edgeFor(width: number, height: number): number | null {
  if (width <= 0 || height <= 0) return null;
  const wanted = Math.ceil(Math.max(width, height) / EDGE_STEP) * EDGE_STEP;
  return Math.min(RELEASE_EDGE, Math.max(MIN_EDGE, wanted));
}

/**
 * The long edge to ask for while the reference follows the current frame's zoom: the reference
 * picture's long edge on screen at that zoom (the zoom's unit is the picture at `PROXY_EDGE`),
 * in the same steps and limits as `edgeFor`.
 */
export function zoomedEdge(zoom: number): number {
  // The small allowance keeps a product like 1024.0000000001 from asking for the next step.
  const wanted = Math.ceil((zoom * PROXY_EDGE) / EDGE_STEP - 1e-6) * EDGE_STEP;
  return Math.min(RELEASE_EDGE, Math.max(MIN_EDGE, wanted));
}

export type ReferenceState =
  | { id: string; status: 'loading'; preview: null; error: null }
  | { id: string; status: 'ready'; preview: Preview; error: null }
  | { id: string; status: 'error'; preview: Preview | null; error: string };

const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * Renders the reference frame with its own saved recipe (the one `developOpen` gives, which is
 * Develop's own starting point for a frame that has none saved). Nothing is ever saved for it.
 * It is rendered again only when the reference, the `edge` or the `revision` changes (the
 * revision is bumped when the reference's saved recipe may have changed, which reads it again); a picture older than the
 * newest one taken, or one that arrives after the reference changed or the pane closed, is dropped.
 * `nextSeq` is the Develop view's counter, so the backend sees one increasing series per frame even
 * when two frames swap places.
 */
export function useReferenceRender(
  id: string,
  edge: number | null,
  nextSeq: () => number,
  revision = 0,
): ReferenceState {
  const [state, setState] = useState<ReferenceState>({
    id,
    status: 'loading',
    preview: null,
    error: null,
  });
  const recipe = useRef<{ id: string; revision: number; recipe: Recipe } | null>(null);
  const accepted = useRef({ id, seq: -1 });

  useEffect(() => {
    if (edge === null) return;
    let alive = true;
    if (accepted.current.id !== id) accepted.current = { id, seq: -1 };
    setState((s) => (s.id === id ? s : { id, status: 'loading', preview: null, error: null }));
    void (async () => {
      try {
        if (recipe.current?.id !== id || recipe.current.revision !== revision) {
          const info = await developOpen(id);
          if (!alive) return;
          recipe.current = { id, revision, recipe: info.recipe };
        }
        const source = recipe.current;
        if (!source) return;
        const picture = await developRender(id, source.recipe, edge, nextSeq());
        if (!alive || picture.seq <= accepted.current.seq) return;
        accepted.current.seq = picture.seq;
        setState({ id, status: 'ready', preview: picture, error: null });
      } catch (error) {
        // A render the backend superseded is not a failure: a newer one is on its way.
        if (!alive || (error instanceof IpcError && error.error.code === 'Cancelled')) return;
        setState((s) => ({
          id,
          status: 'error',
          preview: s.id === id ? s.preview : null,
          error: messageOf(error),
        }));
      }
    })();
    return () => {
      alive = false;
    };
  }, [id, edge, nextSeq, revision]);

  return state;
}
