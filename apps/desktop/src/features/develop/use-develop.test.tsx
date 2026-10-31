import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IpcError, type Preview } from '../../shared/ipc';
import { ipc, makePreview, makeRecipe, resetFake, state } from '../library/fake-ipc';
import {
  DRAG_DELAY_MS,
  DRAG_EDGE,
  RELEASE_EDGE,
  SAVE_DELAY_MS,
  WHOLE_SCAN_GEOMETRY,
  useDevelop,
} from './use-develop';

vi.mock('../../shared/ipc', async () => (await import('../library/fake-ipc')).ipc);

/** A promise settled by hand, to decide in which order renders come back. */
function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  let reject: (error: unknown) => void = () => undefined;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const pixel = (preview: Preview | null) => preview?.rgba.slice(0, 3).join(',');

describe('useDevelop', () => {
  beforeEach(() => {
    resetFake();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  describe('opening a frame', () => {
    it('opens it, renders the first picture at the release size, and finds the automatic recipe', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      expect(result.current.status).toBe('opening');
      expect(result.current.preview).toBeNull();

      await waitFor(() => expect(result.current.preview).not.toBeNull());
      expect(result.current.status).toBe('ready');
      expect(result.current.info).toMatchObject({ width: 5616, height: 3744 });
      expect(result.current.recipe).toEqual(makeRecipe('saved'));
      expect(ipc.developOpen).toHaveBeenCalledWith('f1');
      expect(ipc.developRender).toHaveBeenCalledTimes(1);
      expect(ipc.developRender).toHaveBeenCalledWith('f1', makeRecipe('saved'), RELEASE_EDGE, 1);
      expect(result.current.preview).toMatchObject({ seq: 1, width: 128 });
      await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalledWith('f1', null));
    });

    it('says why a frame could not be opened', async () => {
      state.noScan.add('f1');
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.status).toBe('error'));
      expect(result.current.error).toBe('This frame has no scan file.');
      expect(result.current.preview).toBeNull();
      expect(ipc.developRender).not.toHaveBeenCalled();
    });

    it('does nothing without a frame, and forgets a frame when it is closed', async () => {
      const { result, rerender } = renderHook(({ id }) => useDevelop(id), {
        initialProps: { id: null as string | null },
      });
      expect(result.current.status).toBe('idle');
      expect(ipc.developOpen).not.toHaveBeenCalled();

      rerender({ id: 'f1' });
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      rerender({ id: null });
      await waitFor(() => expect(result.current.status).toBe('idle'));
      expect(result.current.preview).toBeNull();
      expect(result.current.info).toBeNull();
    });

    it('starts over for another frame and ignores what the first one still sends', async () => {
      const slow = deferred<Preview>();
      ipc.developRender.mockImplementationOnce(() => slow.promise);
      const { result, rerender } = renderHook(({ id }) => useDevelop(id), {
        initialProps: { id: 'f1' },
      });
      await waitFor(() => expect(ipc.developRender).toHaveBeenCalledTimes(1));

      rerender({ id: 'f2' });
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      const second = result.current.preview;
      expect(ipc.developRender.mock.calls[1]?.[0]).toBe('f2');

      // The first frame's picture arrives late: it is not for this frame.
      await act(async () => {
        slow.resolve(makePreview(makeRecipe('auto'), 2048, 1));
        await slow.promise;
      });
      expect(result.current.preview).toBe(second);
    });
  });

  describe('previews while a recipe changes', () => {
    it('asks for a rough preview at most once per short delay while dragging, and a sharp one on release', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      ipc.developRender.mockClear();
      vi.useFakeTimers();

      // A burst of drag updates: only the last one is asked for, after the delay, at 1600 px.
      act(() => {
        for (const tag of ['a', 'b', 'c']) {
          result.current.setRecipe(makeRecipe(tag), { dragging: true });
        }
      });
      expect(ipc.developRender).not.toHaveBeenCalled();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(DRAG_DELAY_MS + 5);
      });
      expect(ipc.developRender).toHaveBeenCalledTimes(1);
      expect(ipc.developRender).toHaveBeenLastCalledWith('f1', makeRecipe('c'), DRAG_EDGE, 2);

      // Released: a sharp one at once, and a rough one still waiting is dropped.
      act(() => {
        result.current.setRecipe(makeRecipe('d'), { dragging: true });
        result.current.setRecipe(makeRecipe('e'), { dragging: false });
      });
      expect(ipc.developRender).toHaveBeenCalledTimes(2);
      expect(ipc.developRender).toHaveBeenLastCalledWith('f1', makeRecipe('e'), RELEASE_EDGE, 3);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(DRAG_DELAY_MS * 3);
      });
      expect(ipc.developRender).toHaveBeenCalledTimes(2);
      expect(result.current.recipe).toEqual(makeRecipe('e'));
    });

    it('numbers its requests upwards, and only the newest picture is kept', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      const first = deferred<Preview>();
      const second = deferred<Preview>();
      ipc.developRender
        .mockImplementationOnce(() => first.promise)
        .mockImplementationOnce(() => second.promise);

      act(() => {
        result.current.setRecipe(makeRecipe('a'));
        result.current.setRecipe(makeRecipe('b'));
      });
      const seqs = ipc.developRender.mock.calls.slice(-2).map((c) => c[3]);
      expect(seqs[1]).toBeGreaterThan(seqs[0] as number);
      expect(result.current.busy).toBe(true);

      // The newer picture arrives first, then the older one: the older is dropped.
      await act(async () => {
        second.resolve({ ...makePreview(makeRecipe('auto'), 2048, seqs[1] as number) });
        await second.promise;
      });
      expect(pixel(result.current.preview)).toBe('200,100,50');
      await act(async () => {
        first.resolve({ ...makePreview(makeRecipe('saved'), 2048, seqs[0] as number) });
        await first.promise;
      });
      expect(pixel(result.current.preview)).toBe('200,100,50');
      expect(result.current.preview?.seq).toBe(seqs[1]);
      expect(result.current.busy).toBe(false);
    });

    it('does not treat a superseded request as a failure, but reports a real one and keeps the picture', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      const shown = result.current.preview;

      ipc.developRender.mockRejectedValueOnce(
        new IpcError({ code: 'Cancelled', message: 'A newer preview was requested.' }),
      );
      await act(async () => {
        result.current.setRecipe(makeRecipe('a'));
      });
      await waitFor(() => expect(result.current.busy).toBe(false));
      expect(result.current.renderError).toBeNull();
      expect(result.current.preview).toBe(shown);

      ipc.developRender.mockRejectedValueOnce(new Error('The recipe could not be rendered.'));
      await act(async () => {
        result.current.setRecipe(makeRecipe('b'));
      });
      await waitFor(() =>
        expect(result.current.renderError).toBe('The recipe could not be rendered.'),
      );
      expect(result.current.preview).toBe(shown);

      // The next picture clears the message.
      await act(async () => {
        result.current.setRecipe(makeRecipe('c'));
      });
      await waitFor(() => expect(result.current.renderError).toBeNull());
      expect(result.current.preview).not.toBe(shown);
    });
  });

  describe('before and after', () => {
    it('shows the automatic conversion as "before", rendered once and on demand', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalled());
      expect(pixel(result.current.preview)).toBe('10,20,30');
      expect(result.current.showBefore).toBe(false);
      ipc.developRender.mockClear();

      act(() => result.current.toggleBefore());
      await waitFor(() => expect(result.current.showingBefore).toBe(true));
      expect(ipc.developRender).toHaveBeenCalledTimes(1);
      expect(ipc.developRender.mock.calls[0]?.[1]).toMatchObject({ tag: 'auto' });
      expect(ipc.developRender.mock.calls[0]?.[2]).toBe(RELEASE_EDGE);
      expect(pixel(result.current.preview)).toBe('200,100,50');

      // Back to the edit, and again to "before": no new render, both pictures are kept.
      act(() => result.current.toggleBefore());
      expect(pixel(result.current.preview)).toBe('10,20,30');
      act(() => result.current.toggleBefore());
      expect(pixel(result.current.preview)).toBe('200,100,50');
      expect(ipc.developRender).toHaveBeenCalledTimes(1);
    });

    it('keeps showing the edit until the "before" picture has arrived', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalled());
      const slow = deferred<Preview>();
      ipc.developRender.mockImplementationOnce(() => slow.promise);

      act(() => result.current.toggleBefore());
      expect(result.current.showBefore).toBe(true);
      expect(result.current.showingBefore).toBe(false);
      expect(pixel(result.current.preview)).toBe('10,20,30');
      await waitFor(() => expect(ipc.developRender).toHaveBeenCalledTimes(2));

      await act(async () => {
        slow.resolve(makePreview(makeRecipe('auto'), 2048, 2));
        await slow.promise;
      });
      expect(result.current.showingBefore).toBe(true);
    });

    it('says so when the automatic conversion cannot be made', async () => {
      state.noAnalysis.add('f1');
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      act(() => result.current.toggleBefore());
      await waitFor(() => expect(result.current.beforeUnavailable).toBe(true));
      expect(result.current.showingBefore).toBe(false);
      expect(pixel(result.current.preview)).toBe('10,20,30');
    });

    it('is off again for the next frame', async () => {
      const { result, rerender } = renderHook(({ id }) => useDevelop(id), {
        initialProps: { id: 'f1' },
      });
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalled());
      act(() => result.current.toggleBefore());
      await waitFor(() => expect(result.current.showingBefore).toBe(true));

      rerender({ id: 'f2' });
      await waitFor(() => expect(result.current.status).toBe('ready'));
      expect(result.current.showBefore).toBe(false);
      await waitFor(() => expect(pixel(result.current.preview)).toBe('10,20,30'));
    });
  });

  describe('autosave', () => {
    it('saves the recipe once it has been left alone for half a second, whatever changed meanwhile', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      vi.useFakeTimers();

      act(() => result.current.setRecipe(makeRecipe('a')));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(SAVE_DELAY_MS - 200);
      });
      act(() => result.current.setRecipe(makeRecipe('b'), { dragging: true }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(SAVE_DELAY_MS - 100);
      });
      // More than half a second since the first change, but not since the last.
      expect(ipc.recipeSave).not.toHaveBeenCalled();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(200);
      });
      expect(ipc.recipeSave).toHaveBeenCalledTimes(1);
      expect(ipc.recipeSave).toHaveBeenCalledWith('f1', makeRecipe('b'));
      expect(state.saved).toHaveLength(1);
      expect(result.current.saveStatus).toBe('saved');
    });

    it('says it is saving while the save is on its way', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      const slow = deferred<Awaited<ReturnType<typeof ipc.recipeSave>>>();
      ipc.recipeSave.mockImplementationOnce(() => slow.promise);
      vi.useFakeTimers();

      act(() => result.current.setRecipe(makeRecipe('a')));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(SAVE_DELAY_MS + 10);
      });
      expect(result.current.saveStatus).toBe('saving');
      await act(async () => {
        slow.resolve(makeRecipe('a'));
        await slow.promise;
      });
      expect(result.current.saveStatus).toBe('saved');
    });

    it('does not save a frame that was only looked at', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      vi.useFakeTimers();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(SAVE_DELAY_MS * 4);
      });
      expect(ipc.recipeSave).not.toHaveBeenCalled();
      expect(result.current.saveStatus).toBe('idle');
    });

    it('reports a save that failed, and tries again with the next change', async () => {
      state.saveFails = 'The disk is full.';
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      vi.useFakeTimers();

      act(() => result.current.setRecipe(makeRecipe('a')));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(SAVE_DELAY_MS + 10);
      });
      expect(result.current.saveStatus).toBe('error');
      expect(result.current.saveError).toBe('The disk is full.');

      state.saveFails = null;
      act(() => result.current.setRecipe(makeRecipe('b')));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(SAVE_DELAY_MS + 10);
      });
      expect(result.current.saveStatus).toBe('saved');
      expect(state.saved.map((s) => s.recipe)).toEqual([makeRecipe('b')]);
    });

    it('saves an edit that is waiting at once when another frame is opened', async () => {
      const { result, rerender } = renderHook(({ id }) => useDevelop(id), {
        initialProps: { id: 'f1' },
      });
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      act(() => result.current.setRecipe(makeRecipe('a')));
      expect(ipc.recipeSave).not.toHaveBeenCalled();

      rerender({ id: 'f2' });
      expect(ipc.recipeSave).toHaveBeenCalledTimes(1);
      expect(ipc.recipeSave).toHaveBeenCalledWith('f1', makeRecipe('a'));
      await waitFor(() => expect(result.current.status).toBe('ready'));
      expect(result.current.saveStatus).toBe('idle');
    });

    it('saves an edit that is waiting when the view closes', async () => {
      const { result, unmount } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      act(() => result.current.setRecipe(makeRecipe('a')));
      unmount();
      expect(ipc.recipeSave).toHaveBeenCalledWith('f1', makeRecipe('a'));
    });

    it('logs an edit that could not be saved while its frame was being closed', async () => {
      const { result, rerender } = renderHook(({ id }) => useDevelop(id), {
        initialProps: { id: 'f1' },
      });
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      state.saveFails = 'The disk is full.';
      act(() => result.current.setRecipe(makeRecipe('a')));
      rerender({ id: 'f2' });
      await waitFor(() => expect(ipc.logClientEvent).toHaveBeenCalled());
      expect(ipc.logClientEvent).toHaveBeenCalledWith(
        'Warn',
        expect.stringContaining('The disk is full.'),
      );
    });
  });

  describe('the whole scan, for picking', () => {
    const wholeScan = () =>
      ipc.developRender.mock.calls.filter(
        (call) =>
          JSON.stringify((call[1] as { geometry?: unknown }).geometry) ===
          JSON.stringify(WHOLE_SCAN_GEOMETRY),
      );

    it('renders the recipe with no crop or turn, and shows it once it has arrived', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      const edit = result.current.preview;
      expect(wholeScan()).toHaveLength(0);

      act(() => result.current.setScanView(true));
      await waitFor(() => expect(result.current.showingScan).toBe(true));
      expect(wholeScan()).toHaveLength(1);
      const [id, recipe, edge] = wholeScan()[0] ?? [];
      expect([id, edge]).toEqual(['f1', RELEASE_EDGE]);
      // Everything but the geometry is as it is.
      expect(recipe).toMatchObject({ tag: 'saved', base: { source: 'roll' } });
      expect(result.current.preview).not.toBe(edit);

      act(() => result.current.setScanView(false));
      expect(result.current.showingScan).toBe(false);
      expect(result.current.preview).toBe(edit);
    });

    it('keeps showing the picture until the scan has arrived', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      const edit = result.current.preview;
      const slow = deferred<Preview>();
      ipc.developRender.mockImplementationOnce(() => slow.promise);

      act(() => result.current.setScanView(true));
      expect(result.current.showingScan).toBe(false);
      expect(result.current.preview).toBe(edit);
      await act(async () => {
        slow.resolve(makePreview(makeRecipe('saved'), 2048, 99));
        await slow.promise;
      });
      expect(result.current.showingScan).toBe(true);
      expect(result.current.preview?.seq).toBe(99);
    });

    it('shows the recipe as it is each time picking starts, not as it was the last time', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      act(() => result.current.setScanView(true));
      await waitFor(() => expect(result.current.showingScan).toBe(true));
      act(() => result.current.setScanView(false));

      act(() => result.current.setRecipe(makeRecipe('edited')));
      act(() => result.current.setScanView(true));
      await waitFor(() => expect(wholeScan()).toHaveLength(2));
      expect(wholeScan()[1]?.[1]).toMatchObject({ tag: 'edited' });
    });

    it('wins over "before" while it is on', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalled());
      act(() => result.current.toggleBefore());
      await waitFor(() => expect(result.current.showingBefore).toBe(true));

      act(() => result.current.setScanView(true));
      await waitFor(() => expect(result.current.showingScan).toBe(true));
      expect(result.current.showingBefore).toBe(false);
      act(() => result.current.setScanView(false));
      expect(result.current.showingBefore).toBe(true);
    });
  });

  describe('refresh', () => {
    it('reads the frame again and renders the recipe being edited, which is kept', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalledTimes(1));
      act(() => result.current.setRecipe(makeRecipe('edited')));
      ipc.developOpen.mockClear();
      ipc.developRender.mockClear();

      await act(async () => {
        await result.current.refresh();
      });
      expect(ipc.developOpen).toHaveBeenCalledWith('f1');
      await waitFor(() => expect(ipc.developRender).toHaveBeenCalledTimes(1));
      expect(ipc.developRender.mock.calls[0]?.[1]).toMatchObject({ tag: 'edited' });
      expect(result.current.recipe).toMatchObject({ tag: 'edited' });
      await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalledTimes(2));
    });

    it('forgets the "before" picture, which depended on the roll\'s base', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      await waitFor(() => expect(ipc.developAnalyze).toHaveBeenCalled());
      act(() => result.current.toggleBefore());
      await waitFor(() => expect(result.current.showingBefore).toBe(true));
      ipc.developRender.mockClear();

      await act(async () => {
        await result.current.refresh();
      });
      // "Before" is still wanted, so it is rendered again from the new analysis.
      await waitFor(() => expect(result.current.showingBefore).toBe(true));
      const tags = ipc.developRender.mock.calls.map((c) => (c[1] as { tag: string }).tag);
      expect(tags.filter((t) => t === 'auto')).toHaveLength(1);
    });

    it('reports a frame that can no longer be read, and keeps the picture', async () => {
      const { result } = renderHook(() => useDevelop('f1'));
      await waitFor(() => expect(result.current.preview).not.toBeNull());
      const shown = result.current.preview;
      ipc.developOpen.mockRejectedValueOnce(new Error('The original is gone.'));
      await act(async () => {
        await result.current.refresh();
      });
      expect(result.current.renderError).toBe('The original is gone.');
      expect(result.current.preview).toBe(shown);
    });
  });
});
