import { render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { makePreview, makeRecipe } from '../library/fake-ipc';
import { Canvas } from './Canvas';
import { FIT_VIEW, referenceSize } from './viewport';

/** jsdom has no canvas: a context that records what is drawn, and an `ImageData` to carry it. */
class FakeImageData {
  constructor(
    readonly data: Uint8ClampedArray,
    readonly width: number,
    readonly height: number,
  ) {}
}

describe('Canvas', () => {
  const putImageData = vi.fn();

  beforeEach(() => {
    putImageData.mockClear();
    vi.stubGlobal('ImageData', FakeImageData);
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(
      () => ({ putImageData }) as unknown as CanvasRenderingContext2D,
    );
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  const preview = (edge = 2048, seq = 1) => makePreview(makeRecipe('saved'), edge, seq);
  const view = (zoom: number, x = 0, y = 0) => ({ fit: false, zoom, offset: { x, y } });

  it('is a labelled picture that draws the preview pixels at the size they have', () => {
    const picture = preview();
    render(<Canvas preview={picture} label="Frame 3" view={FIT_VIEW} />);
    const canvas = screen.getByRole('img', { name: 'Frame 3' }) as HTMLCanvasElement;
    expect(canvas.width).toBe(picture.width);
    expect(canvas.height).toBe(picture.height);
    expect(putImageData).toHaveBeenCalledTimes(1);
    const [image, x, y] = putImageData.mock.calls[0] as [FakeImageData, number, number];
    expect([x, y]).toEqual([0, 0]);
    expect([image.width, image.height]).toEqual([picture.width, picture.height]);
    expect(image.data).toBe(picture.rgba);
  });

  it('is as big on screen as the zoom says, relative to the proxy size', () => {
    const picture = preview();
    const { rerender } = render(<Canvas preview={picture} label="x" view={view(0.5)} />);
    const canvas = screen.getByRole('img');
    const reference = referenceSize(picture);
    expect(canvas).toHaveStyle({
      width: `${reference.width * 0.5}px`,
      height: `${reference.height * 0.5}px`,
    });

    // A rougher preview of the same frame fills the same area: it is the same picture, less sharp.
    // (The fake previews are tiny, so rounding makes their heights differ by about 1%.)
    rerender(<Canvas preview={preview(1600, 2)} label="x" view={view(0.5)} />);
    expect(canvas.style.width).toBe(`${reference.width * 0.5}px`);
    const height = parseFloat(canvas.style.height);
    expect(Math.abs(height - reference.height * 0.5) / height).toBeLessThan(0.02);
  });

  it('is shifted by the pan offset around the centre of the stage', () => {
    render(<Canvas preview={preview()} label="x" view={view(2, 30, -20)} />);
    expect(screen.getByRole('img').style.transform).toBe(
      'translate(-50%, -50%) translate(30px, -20px)',
    );
  });

  it('enlarges pixels as they are past 100%, and not below', () => {
    const { rerender } = render(<Canvas preview={preview()} label="x" view={view(1)} />);
    expect(screen.getByRole('img').className).not.toMatch(/enlarged/);
    rerender(<Canvas preview={preview()} label="x" view={view(2.5)} />);
    expect(screen.getByRole('img').className).toMatch(/enlarged/);
  });

  it('draws again when a new preview arrives, and not when only the view changes', () => {
    const { rerender } = render(<Canvas preview={preview(1600, 1)} label="x" view={FIT_VIEW} />);
    expect(putImageData).toHaveBeenCalledTimes(1);
    rerender(<Canvas preview={preview(2048, 2)} label="x" view={FIT_VIEW} />);
    expect(putImageData).toHaveBeenCalledTimes(2);
    const [image] = putImageData.mock.calls[1] as [FakeImageData];
    expect(image.width).toBe(128);
    const same = preview(2048, 3);
    rerender(<Canvas preview={same} label="x" view={FIT_VIEW} />);
    rerender(<Canvas preview={same} label="x" view={view(3)} />);
    expect(putImageData).toHaveBeenCalledTimes(3);
  });

  it('copes with a canvas that cannot be drawn on', () => {
    vi.mocked(HTMLCanvasElement.prototype.getContext).mockImplementation(() => null);
    expect(() => render(<Canvas preview={preview()} label="x" view={FIT_VIEW} />)).not.toThrow();
    expect(putImageData).not.toHaveBeenCalled();
  });
});
