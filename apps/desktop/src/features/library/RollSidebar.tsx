import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { baseStatus, baseStatusText } from '../../shared/film-base-status';
import {
  flatFieldClear,
  flatFieldImport,
  importFiles,
  pickCameraFiles,
  pickTiffFiles,
  type Roll,
} from '../../shared/ipc';
import { DeleteRollDialog } from './DeleteRollDialog';
import styles from './library.module.css';
import { messageOf, useLibraryStore } from './library-store';
import { RollEditor } from './RollEditor';

/** The rolls of the library, with actions on the selected one: edit, delete and import scans. */
export function RollSidebar() {
  const { t, i18n } = useTranslation();
  const rolls = useLibraryStore((s) => s.rolls);
  const rollId = useLibraryStore((s) => s.rollId);
  const selectRoll = useLibraryStore((s) => s.selectRoll);
  const readOnly = useLibraryStore((s) => s.library?.read_only ?? false);
  const addJob = useLibraryStore((s) => s.addJob);
  const setNotice = useLibraryStore((s) => s.setNotice);
  const reloadRolls = useLibraryStore((s) => s.reloadRolls);

  const [editing, setEditing] = useState<{ roll: Roll | null } | null>(null);
  const [deleting, setDeleting] = useState<Roll | null>(null);
  const current = rolls.find((r) => r.id === rollId) ?? null;

  const summary = (roll: Roll) =>
    new Intl.ListFormat(i18n.language, { style: 'narrow', type: 'conjunction' }).format(
      [roll.film_stock, roll.camera].filter((v): v is string => !!v),
    );

  const importInto = async (roll: Roll) => {
    try {
      const paths = await pickTiffFiles();
      if (paths.length === 0) return;
      const id = await importFiles(paths, roll.id, 'TiffScan');
      addJob({ id, kind: 'import', rollName: roll.name, fraction: 0, state: 'running' });
    } catch (e) {
      setNotice({ key: 'library.notice.importFailed', values: { message: messageOf(e) } });
    }
  };

  const changeFlatField = async (change: () => Promise<unknown>) => {
    try {
      await change();
      await reloadRolls();
    } catch (e) {
      setNotice({ key: 'library.notice.flatFieldFailed', values: { message: messageOf(e) } });
    }
  };

  const setFlatField = async (roll: Roll) => {
    const paths = await pickCameraFiles();
    if (paths.length === 0) return;
    await changeFlatField(() => flatFieldImport(roll.id, paths));
  };

  return (
    <nav aria-label={t('library.rolls.label')} className={styles.sidebar}>
      <div className={styles.sidebarHead}>
        <h3 className={styles.sidebarTitle}>{t('library.rolls.label')}</h3>
        <button type="button" disabled={readOnly} onClick={() => setEditing({ roll: null })}>
          {t('library.rolls.new')}
        </button>
      </div>

      <ul className={styles.rollList}>
        <li>
          <button
            type="button"
            className={styles.rollButton}
            aria-current={rollId === null ? 'true' : undefined}
            onClick={() => selectRoll(null)}
          >
            {t('library.allFrames')}
          </button>
        </li>
        {rolls.map((roll) => (
          <li key={roll.id}>
            <button
              type="button"
              className={styles.rollButton}
              aria-current={roll.id === rollId ? 'true' : undefined}
              onClick={() => selectRoll(roll.id)}
            >
              <span>{roll.name}</span>
              <small className={styles.rollSummary}>{summary(roll)}</small>
            </button>
          </li>
        ))}
      </ul>
      {rolls.length === 0 && <p className={styles.hint}>{t('library.noRolls')}</p>}

      {current && (
        <div role="group" aria-label={t('library.rolls.actions')} className={styles.actions}>
          <button type="button" disabled={readOnly} onClick={() => setEditing({ roll: current })}>
            {t('library.rolls.edit')}
          </button>
          <button type="button" disabled={readOnly} onClick={() => setDeleting(current)}>
            {t('library.rolls.delete')}
          </button>
          <button type="button" disabled={readOnly} onClick={() => void importInto(current)}>
            {t('library.rolls.import')}
          </button>
          <button type="button" disabled={readOnly} onClick={() => void setFlatField(current)}>
            {t('library.rolls.setFlatField')}
          </button>
          {current.has_flat_field && (
            <button
              type="button"
              disabled={readOnly}
              onClick={() => void changeFlatField(() => flatFieldClear(current.id))}
            >
              {t('library.rolls.removeFlatField')}
            </button>
          )}
        </div>
      )}
      {current && (
        <p className={styles.hint}>
          {baseStatusText(
            baseStatus(current.film_base_rgb !== null, current.film_base_confidence ?? null),
            t,
          )}
        </p>
      )}
      {current && (
        <p className={styles.hint}>
          {t(current.has_flat_field ? 'library.rolls.flatFieldOn' : 'library.rolls.flatFieldOff')}
        </p>
      )}

      <RollEditor
        roll={editing?.roll ?? null}
        open={editing !== null}
        onOpenChange={(open) => !open && setEditing(null)}
      />
      <DeleteRollDialog roll={deleting} onClose={() => setDeleting(null)} />
    </nav>
  );
}
