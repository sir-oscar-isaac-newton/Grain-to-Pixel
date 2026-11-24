/** The most frames a roll holds. */
export const MAX_FRAMES = 40;
/** The resolutions and sample counts the scanner is asked for. */
export const DPIS = [1000, 2000, 4000] as const;
export const SAMPLES = [1, 2, 4, 8, 16] as const;

/** A frame number as typed: a whole number from 1 to 40, or null. */
function frameOf(text: string): number | null {
  if (!/^\d+$/.test(text.trim())) return null;
  const n = Number(text);
  return n >= 1 && n <= MAX_FRAMES ? n : null;
}

/** What is wrong with the frames typed, as translation keys; nothing when they are fine. */
export function frameProblems(first: string, last: string): { first?: string; last?: string } {
  const a = frameOf(first);
  const b = frameOf(last);
  return {
    first: a === null ? 'import.scanner.frameRange' : undefined,
    last:
      b === null
        ? 'import.scanner.frameRange'
        : a !== null && b < a
          ? 'import.scanner.frameOrder'
          : undefined,
  };
}

/** A data URL for the JPEG bytes of the strip. */
export function jpegUrl(bytes: number[]): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i += 8192) {
    binary += String.fromCharCode(...bytes.slice(i, i + 8192));
  }
  return `data:image/jpeg;base64,${btoa(binary)}`;
}
