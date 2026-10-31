import type { ReactNode } from 'react';
import styles from './ProgressList.module.css';

export type ProgressState = 'running' | 'done' | 'error' | 'cancelled';

export interface ProgressItem {
  id: string;
  /** What is running, already translated. */
  label: string;
  /** 0 to 1. */
  fraction: number;
  state: ProgressState;
  /** The state in words ("Importing", "Done", ...), already translated. */
  stateLabel: string;
  /** A result or error line, already translated. */
  detail?: string;
  /** More lines under the detail (for example the files that were skipped). */
  extra?: ReactNode;
}

interface ProgressListProps {
  items: readonly ProgressItem[];
  /** Accessible name of the list, already translated. */
  label: string;
  cancelLabel: string;
  dismissLabel: string;
  onCancel: (id: string) => void;
  onDismiss: (id: string) => void;
}

/** Background jobs with a progress bar each, a cancel button while running and a dismiss after. */
export function ProgressList({
  items,
  label,
  cancelLabel,
  dismissLabel,
  onCancel,
  onDismiss,
}: ProgressListProps) {
  if (items.length === 0) return null;
  return (
    <ul className={styles.list} aria-label={label}>
      {items.map((item) => {
        const percent = Math.round(Math.min(1, Math.max(0, item.fraction)) * 100);
        return (
          <li key={item.id} className={styles.item}>
            <div className={styles.row}>
              <span className={styles.label}>{item.label}</span>
              <span className={styles.state}>{item.stateLabel}</span>
              {item.state === 'running' ? (
                <button type="button" onClick={() => onCancel(item.id)}>
                  {cancelLabel}
                </button>
              ) : (
                <button type="button" onClick={() => onDismiss(item.id)}>
                  {dismissLabel}
                </button>
              )}
            </div>
            <progress
              className={styles.bar}
              max={100}
              value={percent}
              aria-label={item.label}
              data-state={item.state}
            />
            {item.detail && <p className={styles.detail}>{item.detail}</p>}
            {item.extra}
          </li>
        );
      })}
    </ul>
  );
}
