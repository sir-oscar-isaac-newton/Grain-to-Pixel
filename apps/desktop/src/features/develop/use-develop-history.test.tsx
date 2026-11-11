import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Recipe } from '../../shared/ipc';
import { ipc, makeRecipe, resetFake, state } from '../library/fake-ipc';
import { MERGE_WINDOW_MS } from './history';
import { SAVE_DELAY_MS, useDevelop } from './use-develop';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

const edited = (n: number): Recipe => makeRecipe('saved', { tone: { exposure_ev: n } });

async function openFrame(id = 'f1') {
  const hook = renderHook(({ frame }) => useDevelop(frame), { initialProps: { frame: id } });
  await waitFor(() => expect(hook.result.current.preview).not.toBeNull());
  return hook;
}

describe('useDevelop undo history', () => {
  beforeEach(() => {
    resetFake();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('undoes and redoes the changes of a frame', async () => {
    const { result } = await openFrame();
    expect(result.current.canUndo).toBe(false);
    act(() => result.current.setRecipe(edited(1)));
    act(() => result.current.setRecipe(edited(2)));
    expect(result.current.canUndo).toBe(true);
    act(() => result.current.undo());
    expect(result.current.recipe).toEqual(edited(1));
    act(() => result.current.undo());
    expect(result.current.recipe).toEqual(makeRecipe('saved'));
    expect(result.current.canUndo).toBe(false);
    expect(result.current.canRedo).toBe(true);
    act(() => result.current.redo());
    expect(result.current.recipe).toEqual(edited(1));
  });

  it('makes a drag, with its release, one step', async () => {
    const { result } = await openFrame();
    act(() => result.current.setRecipe(edited(1), { dragging: true }));
    act(() => result.current.setRecipe(edited(2), { dragging: true }));
    act(() => result.current.setRecipe(edited(3), { dragging: false }));
    act(() => result.current.undo());
    expect(result.current.recipe).toEqual(makeRecipe('saved'));
    expect(result.current.canUndo).toBe(false);
  });

  it('replaces a recent change of the same field with a reset that asks to merge', async () => {
    const { result } = await openFrame();
    act(() => result.current.setRecipe(edited(1)));
    act(() => result.current.setRecipe(edited(2), { merge: true }));
    act(() => result.current.undo());
    expect(result.current.recipe).toEqual(makeRecipe('saved'));
    expect(result.current.canUndo).toBe(false);
    act(() => result.current.redo());
    expect(result.current.recipe).toEqual(edited(2));
  });

  it('keeps a reset that asks to merge as a step of its own after the window', async () => {
    const { result } = await openFrame();
    const start = Date.now();
    let later = 0;
    vi.spyOn(Date, 'now').mockImplementation(() => start + later);
    act(() => result.current.setRecipe(edited(1)));
    later = MERGE_WINDOW_MS + 1;
    act(() => result.current.setRecipe(edited(2), { merge: true }));
    act(() => result.current.undo());
    expect(result.current.recipe).toEqual(edited(1));
    vi.restoreAllMocks();
  });

  it('counts a change made without a preview as an edit', async () => {
    const { result } = await openFrame();
    act(() => result.current.setRecipe(edited(1), { dragging: true, preview: false }));
    act(() => result.current.setRecipe(edited(2), { dragging: false, preview: false }));
    expect(result.current.canUndo).toBe(true);
    act(() => result.current.undo());
    expect(result.current.recipe).toEqual(makeRecipe('saved'));
  });

  it('previews and saves an undone state like any change, and does not record it as a step', async () => {
    const { result } = await openFrame();
    act(() => result.current.setRecipe(edited(1)));
    await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(1), {
      timeout: SAVE_DELAY_MS * 4,
    });
    ipc.developRender.mockClear();
    act(() => result.current.undo());
    await waitFor(() => expect(ipc.developRender).toHaveBeenCalled());
    expect(ipc.developRender.mock.calls.at(-1)?.[1]).toEqual(makeRecipe('saved'));
    await waitFor(() => expect(ipc.recipeSave).toHaveBeenCalledTimes(2), {
      timeout: SAVE_DELAY_MS * 4,
    });
    expect(state.saved.at(-1)?.recipe).toEqual(makeRecipe('saved'));
    expect(result.current.canUndo).toBe(false);
  });

  it('drops the redo steps when a change follows an undo', async () => {
    const { result } = await openFrame();
    act(() => result.current.setRecipe(edited(1)));
    act(() => result.current.undo());
    expect(result.current.canRedo).toBe(true);
    act(() => result.current.setRecipe(edited(7)));
    expect(result.current.canRedo).toBe(false);
  });

  it('forgets the history when another frame is opened', async () => {
    const { result, rerender } = await openFrame('f1');
    act(() => result.current.setRecipe(edited(1)));
    act(() => result.current.undo());
    act(() => result.current.setRecipe(edited(2)));
    act(() => result.current.undo());
    expect(result.current.canRedo).toBe(true);
    rerender({ frame: 'f2' });
    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(result.current.canUndo).toBe(false);
    expect(result.current.canRedo).toBe(false);
  });
});
