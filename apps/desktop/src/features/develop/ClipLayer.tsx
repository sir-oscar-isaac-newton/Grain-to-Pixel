import { useEffect, useMemo, useRef } from 'react';
import type { Preview } from '../../shared/ipc';
import { pictureStyle } from './Canvas';
import { buildClipMask } from './clip';
import styles from './develop.module.css';
import type { View } from './viewport';

interface ClipLayerProps {
  /** The preview on screen: the layer is computed from these very pixels. */
  preview: Preview;
  view: View;
}

/**
 * The clipping overlay: pixels clipped to white painted red and pixels clipped to black painted
 * blue (rule in `clip.ts`), transparent elsewhere. It sits exactly over the preview, with the same
 * size and place, and its pixels are enlarged as they are, not smoothed. The mask is built once per
 * preview, in one pass. The canvas is decorative: the switch and the legend say what it shows.
 */
export function ClipLayer({ preview, view }: ClipLayerProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  const mask = useMemo(() => buildClipMask(preview.rgba), [preview]);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    canvas.width = preview.width;
    canvas.height = preview.height;
    // No 2D context (a test environment without a canvas): there is nothing to draw on.
    const context = canvas.getContext('2d');
    if (!context || typeof ImageData === 'undefined') return;
    context.imageSmoothingEnabled = false;
    const pixels = mask.rgba as Uint8ClampedArray<ArrayBuffer>;
    context.putImageData(new ImageData(pixels, preview.width, preview.height), 0, 0);
  }, [preview, mask]);

  return (
    <canvas
      ref={ref}
      aria-hidden="true"
      data-layer="clipping"
      className={`${styles.canvas} ${styles.detail} ${styles.clipLayer}`}
      style={pictureStyle(preview, view)}
    />
  );
}
