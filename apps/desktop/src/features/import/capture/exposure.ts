import type { ExposureInfo } from '../../../shared/ipc';

type Ratio = { num: number; den: number };

const sameRatio = (a: Ratio, b: Ratio): boolean => a.num * b.den === b.num * a.den;

/**
 * True when the shutter time, ISO or aperture of two exposures differ. A setting only one side
 * knows is no difference (a picture without EXIF says nothing).
 */
export function exposureDiffers(a: ExposureInfo | null, b: ExposureInfo | null): boolean {
  if (!a || !b) return false;
  const ratios: [Ratio | null, Ratio | null][] = [
    [a.exposure_time, b.exposure_time],
    [a.f_number, b.f_number],
  ];
  return (
    ratios.some(([x, y]) => x !== null && y !== null && !sameRatio(x, y)) ||
    (a.iso !== null && b.iso !== null && a.iso !== b.iso)
  );
}
