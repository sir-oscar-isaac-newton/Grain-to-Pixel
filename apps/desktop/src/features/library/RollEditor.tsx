import * as Dialog from '@radix-ui/react-dialog';
import { useTranslation } from 'react-i18next';
import type { Roll } from '../../shared/ipc';
import styles from './library.module.css';
import { useLibraryStore } from './library-store';
import { RollForm } from './RollForm';

interface RollEditorProps {
  /** The roll to edit, or null to create one. */
  roll: Roll | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** The roll form in a dialog; saving selects the roll in the Library. */
export function RollEditor({ roll, open, onOpenChange }: RollEditorProps) {
  const { t } = useTranslation();
  const selectRoll = useLibraryStore((s) => s.selectRoll);
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content className={styles.dialog}>
          <Dialog.Title>{roll ? t('rollEditor.titleEdit') : t('rollEditor.titleNew')}</Dialog.Title>
          <Dialog.Description>{t('rollEditor.description')}</Dialog.Description>
          {/* Remounted per roll, so the fields always start from that roll. */}
          <RollForm
            key={roll?.id ?? 'new'}
            roll={roll}
            onSaved={(saved) => {
              selectRoll(saved.id);
              onOpenChange(false);
            }}
            onCancel={() => onOpenChange(false)}
          />
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
