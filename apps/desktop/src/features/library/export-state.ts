import type { FrameSummary } from '../../shared/ipc';

/** Where a frame stands with its exports. */
export type ExportState = 'never' | 'current' | 'stale';

/**
 * Never exported, exported and unchanged since, or exported and edited afterwards. Both times are
 * RFC 3339 in UTC from the same clock source in the catalog (`YYYY-MM-DDTHH:MM:SSZ`), so comparing
 * them as text is comparing them in time. An edit within the same second as the export counts as
 * unchanged, as the backend's `needs_export` filter does.
 */
export function exportState(frame: Pick<FrameSummary, 'edited_at' | 'exported_at'>): ExportState {
  if (!frame.exported_at) return 'never';
  return frame.edited_at && frame.edited_at > frame.exported_at ? 'stale' : 'current';
}
