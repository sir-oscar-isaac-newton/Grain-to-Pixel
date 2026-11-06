import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import type { Look } from '../../shared/ipc';
import styles from './settings.module.css';

/** The looks the automatic conversion can start from, in the order they are offered. */
export const LOOKS: readonly Look[] = ['neutral', 'film_log', 'frontier'];

interface StartingLookSectionProps {
  value: Look;
  /** Saving is under way. */
  busy: boolean;
  onChange: (look: Look) => void;
}

/** The Starting look setting: what frames imported from now on start from. */
export function StartingLookSection({ value, busy, onChange }: StartingLookSectionProps) {
  const { t } = useTranslation();
  const id = useId();
  return (
    <section aria-labelledby={`${id}-heading`} className={styles.section}>
      <h3 id={`${id}-heading`} className={styles.heading}>
        {t('settings.look.heading')}
      </h3>
      <p className={styles.muted}>{t('settings.look.intro')}</p>
      <label htmlFor={`${id}-select`}>{t('settings.look.label')}</label>{' '}
      <select
        id={`${id}-select`}
        value={value}
        disabled={busy}
        onChange={(e) => onChange(e.target.value as Look)}
      >
        {LOOKS.map((look) => (
          <option key={look} value={look}>
            {t(`settings.look.${look}`)}
          </option>
        ))}
      </select>
    </section>
  );
}
