import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { updateInstall, updateRestart } from '../../shared/ipc';
import { messageOf } from '../library/library-store';
import styles from './update.module.css';

export interface UpdateOfferInfo {
  version: string;
  current_version: string;
  notes?: string | null;
}

interface UpdateOfferProps {
  offer: UpdateOfferInfo;
  /** The user chose "Later". */
  onLater: () => void;
}

type Phase = 'offered' | 'installing' | 'installed';

/**
 * A newer version, with what is new and the choice to install it. Nothing is installed before the
 * button is pressed. On Windows the installer takes over and closes the app; elsewhere the app
 * asks for a restart when the new version is in place.
 */
export function UpdateOffer({ offer, onLater }: UpdateOfferProps) {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<Phase>('offered');
  const [error, setError] = useState<string | null>(null);

  const install = async () => {
    setPhase('installing');
    setError(null);
    try {
      await updateInstall();
      setPhase('installed');
    } catch (e) {
      setError(t('update.failed', { message: messageOf(e) }));
      setPhase('offered');
    }
  };

  return (
    <div role="status" className={styles.offer}>
      <p>{t('update.available', { version: offer.version, current: offer.current_version })}</p>
      {offer.notes && (
        <>
          <p>{t('update.notes')}</p>
          <pre className={styles.notes}>{offer.notes}</pre>
        </>
      )}
      {phase === 'installing' && <p>{t('update.installing')}</p>}
      {phase === 'installed' && <p>{t('update.installed')}</p>}
      {error && <p role="alert">{error}</p>}
      <div className={styles.buttons}>
        {phase === 'installed' ? (
          <button type="button" onClick={() => void updateRestart()}>
            {t('update.restart')}
          </button>
        ) : (
          <>
            <button type="button" disabled={phase === 'installing'} onClick={() => void install()}>
              {t('update.install')}
            </button>
            <button type="button" disabled={phase === 'installing'} onClick={onLater}>
              {t('update.later')}
            </button>
          </>
        )}
      </div>
    </div>
  );
}
