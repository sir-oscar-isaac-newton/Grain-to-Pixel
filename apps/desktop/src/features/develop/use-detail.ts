import { useCallback, useEffect, useRef, useState } from 'react';
import {
  IpcError,
  developRenderDetail,
  type Detail,
  type Recipe,
  type DetailRegion,
} from '../../shared/ipc';
import { PROBE_REGION, contains, coveredPart, requestRegion, visiblePart } from './detail-region';
import { oneToOneZoom, type Size, type View } from './viewport';

/** The size of the full-size picture, as the backend reported it for a frame. */
export interface FullSize {
  frameId: string;
  width: number;
  height: number;
}

export interface DetailInput {
  frameId: string;
  /** The recipe the canvas shows (the automatic one while "before" is on); null until open. */
  recipe: Recipe | null;
  /**
   * Anything but the recipe that the picture depends on (the frame's settings as opened, which
   * carry the roll's film base): a picture made for another one is not drawn.
   */
  version: object | null;
  /** False while a drag is under way: the picture is not asked for until the change settles. */
  settled: boolean;
  /** False while the crop is edited or something is picked: the preview is the picture then. */
  enabled: boolean;
  /** The view as drawn, the stage it is drawn on and the size of the preview drawn there. */
  view: View;
  stage: Size;
  picture: Size | null;
  /** The size of the full picture once known (kept by the caller, which needs it for the zoom). */
  full: FullSize | null;
  onFull: (size: FullSize) => void;
}

export interface DetailState {
  /** The zoom at which one full-size pixel is one screen pixel; 0 until the size is known. */
  oneToOne: number;
  /** Detail mode is on: the size is known and the view is at 1:1 or closer. */
  active: boolean;
  /** The region to draw over the preview, when it is the current recipe's and nothing is dragged. */
  layer: Detail | null;
  /** Detail mode is on but the layer is not there yet (a drag, a change, or a render under way). */
  rendering: boolean;
  /** Why the last request failed (a replaced request is not a failure). */
  error: string | null;
  /** Asks for the size of the full picture now (a tiny request), as the 1:1 button needs it. */
  learnSize: () => void;
}

interface Held {
  frameId: string;
  recipe: Recipe;
  version: object | null;
  detail: Detail;
}

interface Flight {
  frameId: string;
  recipe: Recipe;
  version: object | null;
  region: DetailRegion;
  probe: boolean;
}

const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** Whether the view is at the 1:1 zoom or past it (to what the readout can tell apart). */
function atOrPast(zoom: number, oneToOne: number): boolean {
  return oneToOne > 0 && zoom >= oneToOne * (1 - 0.0005);
}

/**
 * The full-size detail of the open frame. While the view is at 1:1 or closer, and the recipe has
 * settled, it asks the backend for the visible part of the picture plus a margin (rendered like a
 * full-size export), and again when the view leaves that part. Before the size of the full picture
 * is known, the first zoom past 100% (or {@link DetailState.learnSize}) asks for a tiny region only
 * to learn it. Every request has a sequence number that rises across frames and across visits to
 * the view (the backend remembers the newest per frame), and an answer is used only when it is
 * the newest one asked for, for the frame still open.
 */
export function useDetail(input: DetailInput): DetailState {
  const { frameId, recipe, version, settled, enabled, view, stage, picture, full, onFull } = input;
  const [held, setHeld] = useState<Held | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The frame the size was asked for, and the frame the state above belongs to: after a change of
  // frame the view still holds the old frame's recipe for one render, which must not be asked for.
  const [sizeFor, setSizeFor] = useState<string | null>(null);
  const [armed, setArmed] = useState<string | null>(null);
  const sizeWanted = sizeFor === frameId;
  const generation = useRef(0);
  const lastSeq = useRef(0);
  const flight = useRef<Flight | null>(null);
  const heldRef = useRef<Held | null>(null);
  heldRef.current = held;

  // A new frame (or the view closing) forgets what was asked for the old one.
  useEffect(() => {
    generation.current += 1;
    flight.current = null;
    setHeld(null);
    setError(null);
    setSizeFor(null);
    setArmed(frameId);
    return () => {
      generation.current += 1;
    };
  }, [frameId]);

  const known = full && full.frameId === frameId ? full : null;
  const oneToOne = known ? oneToOneZoom(Math.max(known.width, known.height)) : 0;
  const active = enabled && !view.fit && atOrPast(view.zoom, oneToOne);

  const pictureWidth = picture?.width ?? 0;
  const pictureHeight = picture?.height ?? 0;
  const { zoom, offset } = view;
  const knownWidth = known?.width ?? 0;
  const knownHeight = known?.height ?? 0;
  useEffect(() => {
    if (armed !== frameId || !enabled || !recipe || !settled || pictureWidth <= 0) return;
    let region: DetailRegion;
    let probe = false;
    if (knownWidth <= 0) {
      // The size is not known: only a tiny request can find it out.
      if (!sizeWanted && (view.fit || zoom <= 1)) return;
      if (flight.current?.probe && flight.current.frameId === frameId) return;
      region = PROBE_REGION;
      probe = true;
    } else {
      if (view.fit || !atOrPast(zoom, oneToOneZoom(Math.max(knownWidth, knownHeight)))) return;
      const visible = visiblePart({ fit: false, zoom, offset }, stage, {
        width: pictureWidth,
        height: pictureHeight,
      });
      if (!visible) return;
      const on = heldRef.current;
      const same = (a: { frameId: string; recipe: Recipe; version: object | null }) =>
        a.frameId === frameId && a.recipe === recipe && a.version === version;
      if (on && same(on) && contains(coveredPart(on.detail), visible)) return;
      const flying = flight.current;
      if (flying && !flying.probe && same(flying) && contains(flying.region, visible)) return;
      region = requestRegion(visible, { width: knownWidth, height: knownHeight });
    }

    const gen = generation.current;
    // Rises across frames and visits to the view, as the backend keeps the newest per frame.
    const seq = Math.max(lastSeq.current + 1, Date.now());
    lastSeq.current = seq;
    flight.current = { frameId, recipe, version, region, probe };
    setError(null);
    void (async () => {
      let detail: Detail | null = null;
      let failure: string | null = null;
      try {
        detail = await developRenderDetail(frameId, recipe, region, seq);
      } catch (cause) {
        if (!(cause instanceof IpcError && cause.error.code === 'Cancelled')) {
          failure = messageOf(cause);
        }
      }
      if (gen !== generation.current) return;
      if (seq !== lastSeq.current) return;
      flight.current = null;
      if (failure) setError(failure);
      if (detail) {
        onFull({ frameId, width: detail.fullWidth, height: detail.fullHeight });
        setHeld({ frameId, recipe, version, detail });
      }
    })();
  }, [
    enabled,
    recipe,
    version,
    settled,
    view.fit,
    zoom,
    offset,
    stage,
    pictureWidth,
    pictureHeight,
    knownWidth,
    knownHeight,
    frameId,
    sizeWanted,
    armed,
    onFull,
  ]);

  const learnSize = useCallback(() => setSizeFor(frameId), [frameId]);

  const current = held && held.frameId === frameId ? held : null;
  const layer =
    active && settled && current && current.recipe === recipe && current.version === version
      ? current.detail
      : null;
  return {
    oneToOne,
    active,
    layer,
    rendering: active && layer === null && error === null,
    error: enabled ? error : null,
    learnSize,
  };
}
