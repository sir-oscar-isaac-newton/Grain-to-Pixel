import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
// The texts ship with the app: the licence of the app itself, the credits that need words of their own,
// and (loaded only when asked for, it is large) the licence text of every component.
import licence from '../../../../../LICENSE?raw';
import notices from '../../../../../docs/third-party-notices.md?raw';
import { messageOf } from '../library/library-store';
import styles from './settings.module.css';

/** The licence texts of all components, as a separate chunk loaded on demand. */
const loadComponentLicences = () =>
  import('../../../../../docs/third-party-licenses.txt?raw').then((m) => m.default);

interface AboutSectionProps {
  /** Loads the component licences; the real texts unless a test gives its own. */
  load?: () => Promise<string>;
}

/** About and licences: the app's licence, the third-party credits and every component's licence. */
export function AboutSection({ load = loadComponentLicences }: AboutSectionProps) {
  const { t } = useTranslation();
  const id = useId();
  const [components, setComponents] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const toggled = (open: boolean) => {
    if (!open || components !== null) return;
    setError(null);
    setLoading(true);
    load()
      .then(setComponents, (e: unknown) =>
        setError(t('settings.about.componentsFailed', { message: messageOf(e) })),
      )
      .finally(() => setLoading(false));
  };

  return (
    <section aria-labelledby={`${id}-heading`} className={styles.section}>
      <h3 id={`${id}-heading`} className={styles.heading}>
        {t('settings.about.heading')}
      </h3>
      <p className={styles.muted}>{t('settings.about.rawler')}</p>

      <details>
        <summary>{t('settings.about.licence')}</summary>
        <pre className={styles.licenceText}>{licence}</pre>
      </details>
      <details>
        <summary>{t('settings.about.notices')}</summary>
        <pre className={styles.licenceText}>{notices}</pre>
      </details>
      <details onToggle={(e) => toggled(e.currentTarget.open)}>
        <summary>{t('settings.about.components')}</summary>
        {loading && (
          <p role="status" className={styles.muted}>
            {t('settings.loading')}
          </p>
        )}
        {error && <p role="alert">{error}</p>}
        {components !== null && <pre className={styles.licenceText}>{components}</pre>}
      </details>
    </section>
  );
}
