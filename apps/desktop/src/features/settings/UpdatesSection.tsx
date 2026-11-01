import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { updateCheck } from '../../shared/ipc';
import { messageOf } from '../library/library-store';
import { UpdateOffer, type UpdateOfferInfo } from '../update/UpdateOffer';
import styles from './settings.module.css';

interface UpdatesSectionProps {
  /** The `update_check` flag: the button does nothing while it is off. */
  enabled: boolean;
}

type Result = { kind: 'upToDate' } | { kind: 'offer'; offer: UpdateOfferInfo } | { kind: 'failed' };

/** A button that asks for an update now, ignoring the limit of one check a day. */
export function UpdatesSection({ enabled }: UpdatesSectionProps) {
  const { t } = useTranslation();
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<Result | null>(null);
  const [error, setError] = useState<string | null>(null);

  const check = async () => {
    setChecking(true);
    setResult(null);
    setError(null);
    try {
      const outcome = await updateCheck(true);
      if (outcome.status === 'available') setResult({ kind: 'offer', offer: outcome });
      else if (outcome.status === 'up_to_date') setResult({ kind: 'upToDate' });
    } catch (e) {
      setResult({ kind: 'failed' });
      setError(t('settings.updates.failed', { message: messageOf(e) }));
    } finally {
      setChecking(false);
    }
  };

  return (
    <section aria-labelledby="settings-updates" className={styles.section}>
      <h3 id="settings-updates" className={styles.heading}>
        {t('settings.updates.heading')}
      </h3>
      <p className={styles.muted}>{t('settings.updates.intro')}</p>
      {!enabled && <p className={styles.muted}>{t('settings.updates.off')}</p>}
      <div className={styles.buttons}>
        <button type="button" disabled={!enabled || checking} onClick={() => void check()}>
          {checking ? t('settings.updates.checking') : t('settings.updates.check')}
        </button>
      </div>
      {result?.kind === 'upToDate' && <p role="status">{t('settings.updates.upToDate')}</p>}
      {result?.kind === 'offer' && (
        <UpdateOffer offer={result.offer} onLater={() => setResult(null)} />
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
