import type { ReactNode } from 'react';
import styles from './StatusBadge.module.css';

export type BadgeTone = 'neutral' | 'ok' | 'warn' | 'error';

interface StatusBadgeProps {
  tone?: BadgeTone;
  children: ReactNode;
}

/** A short status label. The meaning is in the text; the tone only adds emphasis. */
export function StatusBadge({ tone = 'neutral', children }: StatusBadgeProps) {
  return <span className={`${styles.badge} ${styles[tone] ?? ''}`}>{children}</span>;
}
