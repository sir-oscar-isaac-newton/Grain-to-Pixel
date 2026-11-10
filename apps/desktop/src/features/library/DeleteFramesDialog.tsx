import * as AlertDialog from '@radix-ui/react-alert-dialog';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useUiStore } from '../../app/store';
import { frameDelete } from '../../shared/ipc';
import styles from './library.module.css';
import { messageOf, useLibraryStore } from './library-store';

interface DeleteFramesDialogProps {
  /** The frames to delete. */
  frameIds: string[];
  open: boolean;
  onClose: () => void;
}

/** Asks before deleting frames, saying that their original files go too and cannot come back. */
export function DeleteFramesDialog({ frameIds, open, onClose }: DeleteFramesDialogProps) {
  const { t } = useTranslation();
  const reloadRolls = useLibraryStore((s) => s.reloadRolls);
  const setSelected = useLibraryStore((s) => s.setSelected);
  const bump = useLibraryStore((s) => s.bump);
  const setNotice = useLibraryStore((s) => s.setNotice);
  const closeDevelopFrame = useUiStore((s) => s.closeDevelopFrame);
  const [busy, setBusy] = useState(false);

  const confirm = async () => {
    setBusy(true);
    try {
      const report = await frameDelete(frameIds);
      setSelected([]);
      closeDevelopFrame(frameIds);
      bump();
      await reloadRolls();
      setNotice(
        report.files_left.length > 0
          ? {
              key: 'library.notice.framesDeletedLeft',
              values: { count: report.files_left.length, files: report.files_left.join(', ') },
            }
          : { key: 'library.notice.framesDeleted', values: { count: report.frames_deleted } },
      );
    } catch (e) {
      setNotice({ key: 'library.notice.framesDeleteFailed', values: { message: messageOf(e) } });
    } finally {
      setBusy(false);
      onClose();
    }
  };

  return (
    <AlertDialog.Root open={open} onOpenChange={(next) => !next && !busy && onClose()}>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className={styles.overlay} />
        <AlertDialog.Content className={styles.dialog}>
          <AlertDialog.Title>{t('deleteFrames.title')}</AlertDialog.Title>
          <AlertDialog.Description>
            {t('deleteFrames.body', { count: frameIds.length })}
          </AlertDialog.Description>
          <div className={styles.buttons}>
            <AlertDialog.Cancel asChild>
              <button type="button" disabled={busy}>
                {t('deleteFrames.cancel')}
              </button>
            </AlertDialog.Cancel>
            <button
              type="button"
              className={styles.danger}
              disabled={busy}
              onClick={() => void confirm()}
            >
              {t('deleteFrames.confirm')}
            </button>
          </div>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}
