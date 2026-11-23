import * as Dialog from '@radix-ui/react-dialog';
import { useEffect, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { flatFieldList, type FlatFieldSummary, type Roll } from '../../shared/ipc';
import { formatAperture } from '../import/capture/exposure';
import styles from './library.module.css';
import { messageOf } from './library-store';

interface UseFlatFieldDialogProps {
  /** The roll that takes the flat field; the dialog is open while this is set. */
  roll: Roll | null;
  onClose: () => void;
  /** Copies the flat field of the roll with this id to `roll`; reports its own failures. */
  onCopy: (fromRollId: string, roll: Roll) => Promise<void>;
}

/**
 * Lists the other rolls that have a flat field, newest first, and copies the chosen one to the
 * roll (H-26). Says so when that replaces the flat field the roll has.
 */
export function UseFlatFieldDialog({ roll, onClose, onCopy }: UseFlatFieldDialogProps) {
  const { t, i18n } = useTranslation();
  const [list, setList] = useState<FlatFieldSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const rollId = roll?.id ?? null;
  useEffect(() => {
    setList(null);
    setError(null);
    setChosen(null);
    if (rollId === null) return;
    let alive = true;
    flatFieldList()
      .then((all) => {
        if (alive) setList(all.filter((f) => f.roll_id !== rollId));
      })
      .catch((e: unknown) => {
        if (alive) setError(t('library.useFlatField.loadFailed', { message: messageOf(e) }));
      });
    return () => {
      alive = false;
    };
  }, [rollId, t]);

  const date = (iso: string) => {
    const when = new Date(iso);
    return Number.isNaN(when.getTime())
      ? iso
      : new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' }).format(when);
  };

  const copy = async (event: FormEvent) => {
    event.preventDefault();
    if (!roll || chosen === null || busy) return;
    setBusy(true);
    try {
      await onCopy(chosen, roll);
    } finally {
      setBusy(false);
      onClose();
    }
  };

  return (
    <Dialog.Root open={roll !== null} onOpenChange={(open) => !open && !busy && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content className={styles.dialog}>
          <Dialog.Title>{t('library.useFlatField.title')}</Dialog.Title>
          <Dialog.Description>
            {t('library.useFlatField.description', { roll: roll?.name ?? '' })}
          </Dialog.Description>
          {roll?.has_flat_field && (
            <p role="note">{t('library.useFlatField.replaces', { roll: roll.name })}</p>
          )}

          <form onSubmit={copy} className={styles.form}>
            {error !== null ? (
              <p role="alert">{error}</p>
            ) : list === null ? (
              <p role="status">{t('library.useFlatField.loading')}</p>
            ) : list.length === 0 ? (
              <p role="status">{t('library.useFlatField.none')}</p>
            ) : (
              <fieldset className={styles.fieldset}>
                <legend>{t('library.useFlatField.choose')}</legend>
                {list.map((entry) => (
                  <label key={entry.roll_id} className={styles.radio}>
                    <input
                      type="radio"
                      name="flat-field-source"
                      value={entry.roll_id}
                      checked={chosen === entry.roll_id}
                      onChange={() => setChosen(entry.roll_id)}
                    />
                    {t('library.useFlatField.entry', {
                      name: entry.roll_name,
                      date: date(entry.created_at),
                      aperture:
                        formatAperture(entry.exposure.f_number) ??
                        t('library.useFlatField.apertureUnknown'),
                    })}
                  </label>
                ))}
              </fieldset>
            )}
            <div className={styles.buttons}>
              <button type="submit" disabled={chosen === null || busy}>
                {t('library.useFlatField.copy')}
              </button>
              <Dialog.Close asChild>
                <button type="button" disabled={busy}>
                  {t('library.useFlatField.cancel')}
                </button>
              </Dialog.Close>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
