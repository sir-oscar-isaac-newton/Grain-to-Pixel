import * as AlertDialog from '@radix-ui/react-alert-dialog';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { frameList, rollDelete, type Roll } from '../../shared/ipc';
import styles from './library.module.css';
import { messageOf, useLibraryStore } from './library-store';

interface DeleteRollDialogProps {
  /** The roll to delete; the dialog is open while this is set. */
  roll: Roll | null;
  onClose: () => void;
}

/** Asks before deleting a roll, saying how many frames (and their original files) go with it. */
export function DeleteRollDialog({ roll, onClose }: DeleteRollDialogProps) {
  const { t } = useTranslation();
  const reloadRolls = useLibraryStore((s) => s.reloadRolls);
  const selectRoll = useLibraryStore((s) => s.selectRoll);
  const bump = useLibraryStore((s) => s.bump);
  const setNotice = useLibraryStore((s) => s.setNotice);
  const [count, setCount] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setCount(null);
    if (!roll) return;
    let alive = true;
    frameList({ roll_id: roll.id, page: 0, page_size: 1 })
      .then((page) => {
        if (alive) setCount(page.total);
      })
      .catch(() => {
        // The count is only for the message; deleting still works without it.
        if (alive) setCount(0);
      });
    return () => {
      alive = false;
    };
  }, [roll]);

  const confirm = async () => {
    if (!roll) return;
    setBusy(true);
    try {
      const report = await rollDelete(roll.id);
      selectRoll(null);
      await reloadRolls();
      bump();
      setNotice(
        report.files_left.length > 0
          ? {
              key: 'library.notice.rollDeletedLeft',
              values: { count: report.files_left.length, files: report.files_left.join(', ') },
            }
          : { key: 'library.notice.rollDeleted' },
      );
    } catch (e) {
      setNotice({ key: 'library.notice.deleteFailed', values: { message: messageOf(e) } });
    } finally {
      setBusy(false);
      onClose();
    }
  };

  return (
    <AlertDialog.Root open={roll !== null} onOpenChange={(open) => !open && !busy && onClose()}>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className={styles.overlay} />
        <AlertDialog.Content className={styles.dialog}>
          <AlertDialog.Title>{t('deleteRoll.title')}</AlertDialog.Title>
          <AlertDialog.Description>
            {roll && count !== null
              ? t('deleteRoll.body', { name: roll.name, count })
              : t('deleteRoll.counting')}
          </AlertDialog.Description>
          <div className={styles.buttons}>
            <AlertDialog.Cancel asChild>
              <button type="button" disabled={busy}>
                {t('deleteRoll.cancel')}
              </button>
            </AlertDialog.Cancel>
            <button
              type="button"
              className={styles.danger}
              disabled={busy || count === null}
              onClick={() => void confirm()}
            >
              {t('deleteRoll.confirm')}
            </button>
          </div>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}
