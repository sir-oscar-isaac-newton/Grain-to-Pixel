import {
  edgeMask,
  laplacianVariance,
  luminanceOf,
  paintMask,
  PEAK_COLOURS,
  sharpnessRegion,
  thresholdFor,
  type PeakColour,
} from './focus';

/** What to compute for one frame. */
export interface AnalyseOptions {
  /** Outline the edges on the overlay. */
  peaking: boolean;
  sensitivity: number;
  colour: PeakColour;
  /** The camera zoom is on: the sharpness is read from the whole frame. */
  zoomed: boolean;
}

/** True when frames can be decoded here (not in a test environment without images). */
export function canAnalyse(): boolean {
  return typeof createImageBitmap === 'function';
}

/**
 * Decodes live view frames and measures them. The buffers are made once for the frame size and
 * reused, so a frame costs one pass over typed arrays and no allocation per pixel.
 */
export function createAnalyser() {
  let scratch: HTMLCanvasElement | null = null;
  let lum = new Float32Array(0);
  let mask = new Uint8Array(0);
  let overlayData: ImageData | null = null;

  return {
    /** The sharpness of the frame, or null when it cannot be decoded; draws peaking on `overlay`. */
    async analyse(
      frame: Blob,
      options: AnalyseOptions,
      overlay: HTMLCanvasElement | null,
    ): Promise<number | null> {
      const bitmap = await createImageBitmap(frame);
      const { width, height } = bitmap;
      scratch ??= document.createElement('canvas');
      if (scratch.width !== width || scratch.height !== height) {
        scratch.width = width;
        scratch.height = height;
      }
      const context = scratch.getContext('2d', { willReadFrequently: true });
      if (!context) {
        bitmap.close();
        return null;
      }
      context.drawImage(bitmap, 0, 0);
      bitmap.close();
      const pixels = context.getImageData(0, 0, width, height);
      if (lum.length !== width * height) {
        lum = new Float32Array(width * height);
        mask = new Uint8Array(width * height);
        overlayData = null;
      }
      luminanceOf(pixels.data, lum);
      const sharpness = laplacianVariance(
        lum,
        width,
        height,
        sharpnessRegion(width, height, options.zoomed),
      );
      const target = overlay?.getContext('2d');
      if (overlay && target) {
        if (options.peaking) {
          if (overlay.width !== width || overlay.height !== height) {
            overlay.width = width;
            overlay.height = height;
          }
          overlayData ??= target.createImageData(width, height);
          edgeMask(lum, width, height, thresholdFor(options.sensitivity), mask);
          paintMask(mask, overlayData.data, PEAK_COLOURS[options.colour]);
          target.putImageData(overlayData, 0, 0);
        } else {
          target.clearRect(0, 0, overlay.width, overlay.height);
        }
      }
      return sharpness;
    },
  };
}
