import type { TFunction } from 'i18next';

/** An automatic film base below this confidence is a guess (the backend's own limit). */
export const BASE_GUESS_BELOW = 0.5;

/** How a roll's film base was set. */
export type BaseStatus =
  | { kind: 'none' }
  | { kind: 'hand' }
  | { kind: 'auto'; percent: number }
  | { kind: 'guess'; percent: number };

/**
 * The status of a roll's base: `confidence` is null for a base picked by hand (or no base), and a
 * number from 0 to 1 for an automatic measurement.
 */
export function baseStatus(hasBase: boolean, confidence: number | null): BaseStatus {
  if (!hasBase) return { kind: 'none' };
  if (confidence === null) return { kind: 'hand' };
  const percent = Math.round(confidence * 100);
  return confidence < BASE_GUESS_BELOW ? { kind: 'guess', percent } : { kind: 'auto', percent };
}

/** The status in words; a guess also says where to pick the base. */
export function baseStatusText(status: BaseStatus, t: TFunction): string {
  return status.kind === 'auto' || status.kind === 'guess'
    ? t(`filmBase.status.${status.kind}`, { percent: status.percent })
    : t(`filmBase.status.${status.kind}`);
}
