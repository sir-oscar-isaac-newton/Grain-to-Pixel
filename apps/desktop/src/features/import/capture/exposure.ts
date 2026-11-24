import type { CameraSetting, ExposureInfo } from '../../../shared/ipc';

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

const isRatio = (r: Ratio | null): r is Ratio =>
  r !== null && Number.isFinite(r.num) && Number.isFinite(r.den) && r.num > 0 && r.den > 0;

/** A number with at most `digits` decimals and no trailing zeros ("8", "5.6", "1.5"). */
const plain = (n: number, digits: number): string => String(Number(n.toFixed(digits)));

/** A shutter time as a photographer writes it: "2 s", "1/100 s", "0.3 s". */
function formatShutter(time: Ratio): string {
  const seconds = time.num / time.den;
  if (seconds >= 1) return `${plain(seconds, 1)} s`;
  if (time.den % time.num === 0) return `1/${time.den / time.num} s`;
  return `${plain(seconds, 2)} s`;
}

/** An aperture as a photographer writes it ("f/8", "f/5.6"); null when it is unknown. */
export function formatAperture(aperture: Ratio | null | undefined): string | null {
  if (!aperture || !isRatio(aperture)) return null;
  return `f/${plain(aperture.num / aperture.den, 1)}`;
}

/**
 * The shutter time, ISO and aperture of an exposure as one line ("1/100 s, ISO 100, f/8"), leaving
 * out what is unknown; null when nothing is known. These are photographic notation, the same in
 * every language.
 */
export function formatExposure(exposure: ExposureInfo | null): string | null {
  if (!exposure) return null;
  const parts: string[] = [];
  if (isRatio(exposure.exposure_time)) parts.push(formatShutter(exposure.exposure_time));
  if (exposure.iso !== null) parts.push(`ISO ${exposure.iso}`);
  const aperture = formatAperture(exposure.f_number);
  if (aperture) parts.push(aperture);
  return parts.length > 0 ? parts.join(', ') : null;
}

/** Third-stop shutter denominators from 1/4000 s (the fastest of the D600) down to 1/3 s. */
const FAST_DENOMINATORS = [
  4000, 3200, 2500, 2000, 1600, 1250, 1000, 800, 640, 500, 400, 320, 250, 200, 160, 125, 100, 80,
  60, 50, 40, 30, 25, 20, 15, 13, 10, 8, 6, 5, 4, 3,
];

/** Standard shutter times in seconds, from 0.4 s up. */
const LONG_SECONDS = [
  0.4, 0.5, 0.6, 0.8, 1, 1.3, 1.6, 2, 2.5, 3, 4, 5, 6, 8, 10, 13, 15, 20, 25, 30,
];

/** The value of `candidates` nearest to `x` on a logarithmic scale, as stops are. */
const nearestLog = (x: number, candidates: number[]): number =>
  candidates.reduce((best, c) =>
    Math.abs(Math.log(x / c)) < Math.abs(Math.log(x / best)) ? c : best,
  );

/**
 * A shutter choice of the camera ("0.0200s") as a photographer writes it ("1/50 s"). The camera
 * rounds its times, so they snap to the nearest standard third stop. Text that is not a time
 * ("Bulb") is returned as it is.
 */
export function formatShutterChoice(choice: string): string {
  const match = /^(\d+(?:\.\d+)?)s$/.exec(choice);
  const seconds = match ? Number(match[1]) : NaN;
  if (!Number.isFinite(seconds) || seconds <= 0) return choice;
  if (seconds < 0.4) {
    const nearest = nearestLog(
      seconds,
      FAST_DENOMINATORS.map((d) => 1 / d),
    );
    return `1/${Math.round(1 / nearest)} s`;
  }
  if (seconds > 30) return `${plain(seconds, 2)} s`;
  return `${plain(nearestLog(seconds, LONG_SECONDS), 1)} s`;
}

/** The camera's current ISO, shutter speed and aperture, as it reports them; null where unknown. */
export interface CameraValues {
  iso: string | null;
  shutter: string | null;
  aperture: string | null;
}

/** The aperture of a camera setting ("8", "5.6") as photographers write it ("f/8"); null if unknown. */
export function apertureOfSetting(value: string | null | undefined): string | null {
  if (value === null || value === undefined) return null;
  const n = Number(value.replace(/^f\/?/i, ''));
  return Number.isFinite(n) ? formatAperture({ num: n, den: 1 }) : null;
}

/** True when the camera's aperture is known and is not the aperture the flat field was taken at. */
export function apertureDiffers(
  camera: CameraValues | null,
  flat: Ratio | null | undefined,
): boolean {
  const a = apertureOfSetting(camera?.aperture);
  const b = formatAperture(flat);
  return a !== null && b !== null && a !== b;
}

/**
 * True when the camera's aperture, shutter time or ISO differ from the exposure the levels were
 * saved or calibrated with. What either side does not know is no difference.
 */
export function cameraDiffers(camera: CameraValues | null, exposure: ExposureInfo | null): boolean {
  if (!camera || !exposure) return false;
  if (apertureDiffers(camera, exposure.f_number)) return true;
  const iso = camera.iso === null ? NaN : Number(camera.iso);
  if (exposure.iso !== null && Number.isFinite(iso) && iso !== exposure.iso) return true;
  if (camera.shutter !== null && isRatio(exposure.exposure_time) && /^\d/.test(camera.shutter)) {
    const mine = formatShutterChoice(camera.shutter);
    const theirs = formatExposure({
      exposure_time: exposure.exposure_time,
      iso: null,
      f_number: null,
    });
    return theirs !== null && mine !== theirs;
  }
  return false;
}

/** The camera values in a list of settings as the camera reports them. */
export function valuesOf(settings: CameraSetting[]): CameraValues {
  const value = (kind: CameraSetting['kind']) =>
    settings.find((s) => s.kind === kind)?.value ?? null;
  return { iso: value('iso'), shutter: value('shutter_speed'), aperture: value('aperture') };
}
