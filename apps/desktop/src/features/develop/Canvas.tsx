import { useEffect, useRef } from 'react';
import type { Preview } from '../../shared/ipc';
import styles from './develop.module.css';
import { referenceSize, type View } from './viewport';

interface CanvasProps {
  preview: Preview;
  /** Names the picture for assistive technology. */
  label: string;
  view: View;
}

/**
 * Draws a rendered preview, scaled and shifted as the view says. The picture is plain pixels on a
 * canvas: a 1600 px and a 2048 px preview of the same frame fill the same area, and past 100%
 * pixels are enlarged, not smoothed, so nothing is invented.
 */
export function Canvas({ preview, label, view }: CanvasProps) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    canvas.width = preview.width;
    canvas.height = preview.height;
    // No 2D context (a test environment without a canvas): there is nothing to draw on.
    const context = canvas.getContext('2d');
    if (!context || typeof ImageData === 'undefined') return;
    const pixels = preview.rgba as Uint8ClampedArray<ArrayBuffer>;
    context.putImageData(new ImageData(pixels, preview.width, preview.height), 0, 0);
  }, [preview]);

  const reference = referenceSize(preview);
  return (
    <canvas
      ref={ref}
      role="img"
      aria-label={label}
      className={`${styles.canvas} ${view.zoom > 1 ? styles.enlarged : ''}`}
      style={{
        width: reference.width * view.zoom,
        height: reference.height * view.zoom,
        transform: `translate(-50%, -50%) translate(${view.offset.x}px, ${view.offset.y}px)`,
      }}
    />
  );
}
