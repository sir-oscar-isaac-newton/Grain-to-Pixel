import { useEffect, useRef } from 'react';
import type { Detail } from '../../shared/ipc';
import styles from './develop.module.css';
import { PROXY_EDGE, type View } from './viewport';

interface DetailLayerProps {
  detail: Detail;
  /** Names the picture for assistive technology. */
  label: string;
  view: View;
}

/**
 * Draws a full-size region over the enlarged preview. One pixel of the region covers
 * `zoom * PROXY_EDGE / long edge of the full picture` screen pixels (1 at the 1:1 zoom), and the
 * region sits where it lies in the full picture, which is centred on the stage like the preview.
 * Pixels are enlarged as they are, not smoothed.
 */
export function DetailLayer({ detail, label, view }: DetailLayerProps) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    canvas.width = detail.width;
    canvas.height = detail.height;
    // No 2D context (a test environment without a canvas): there is nothing to draw on.
    const context = canvas.getContext('2d');
    if (!context || typeof ImageData === 'undefined') return;
    const pixels = detail.rgba as Uint8ClampedArray<ArrayBuffer>;
    context.putImageData(new ImageData(pixels, detail.width, detail.height), 0, 0);
  }, [detail]);

  const scale = (view.zoom * PROXY_EDGE) / Math.max(detail.fullWidth, detail.fullHeight);
  const dx = (detail.x + detail.width / 2 - detail.fullWidth / 2) * scale;
  const dy = (detail.y + detail.height / 2 - detail.fullHeight / 2) * scale;
  return (
    <canvas
      ref={ref}
      role="img"
      aria-label={label}
      className={`${styles.canvas} ${styles.detail} ${scale > 1.0005 ? styles.enlarged : ''}`}
      style={{
        width: detail.width * scale,
        height: detail.height * scale,
        transform: `translate(-50%, -50%) translate(${view.offset.x + dx}px, ${view.offset.y + dy}px)`,
      }}
    />
  );
}
