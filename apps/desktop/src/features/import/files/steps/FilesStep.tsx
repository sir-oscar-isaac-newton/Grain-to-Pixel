import { useTranslation } from 'react-i18next';
import styles from '../../import.module.css';
import { StepSection } from '../../capture/steps/StepSection';
import { fileName, isComplete, moveFile, type Group } from '../../triplets';
import { useFiles, type FilesKind } from '../FilesProvider';

const CHANNELS = ['red', 'green', 'blue'] as const;
const KINDS: readonly FilesKind[] = ['camera', 'tiff'];

/**
 * Step "Files": the kind of file, the files chosen and what the check of them found. Camera files
 * are grouped into triplets by capture time, the order of each one can be fixed and the ones to
 * import confirmed; TIFF scans are single files, one frame each.
 */
export function FilesStep() {
  const { t, i18n } = useTranslation();
  const files = useFiles();
  const { kind, groups, tiffPaths } = files;

  const when = (iso: string | null | undefined) => {
    if (!iso) return t('import.files.noTime');
    const date = new Date(iso);
    return Number.isNaN(date.getTime())
      ? iso
      : new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'medium' }).format(
          date,
        );
  };

  const update = (id: number, change: (group: Group) => Group) =>
    files.setGroups((all) => all.map((g) => (g.id === id ? change(g) : g)));

  const empty = kind === 'camera' ? groups.length === 0 : tiffPaths.length === 0;

  return (
    <StepSection id="import-files" heading={t('import.steps.files.heading')}>
      <p className={styles.muted}>{t('import.steps.files.intro')}</p>

      <fieldset className={styles.choices}>
        <legend>{t('import.files.kind.legend')}</legend>
        {KINDS.map((k) => (
          <label key={k} className={styles.choice}>
            <input
              type="radio"
              name="import-files-kind"
              checked={kind === k}
              onChange={() => files.setKind(k)}
            />
            <span>{t(`import.files.kind.${k}`)}</span>
          </label>
        ))}
      </fieldset>

      <div className={styles.row}>
        <button
          type="button"
          disabled={files.busy || files.readOnly}
          onClick={() => void files.choose()}
        >
          {files.busy && empty
            ? t('import.files.choosing')
            : t(kind === 'camera' ? 'import.files.choose' : 'import.files.chooseTiff')}
        </button>
      </div>

      {files.chooseError && <p role="alert">{files.chooseError}</p>}

      {kind === 'tiff' && tiffPaths.length > 0 && (
        <>
          <p role="status" className={styles.summary}>
            {t('import.files.tiffSummary', { count: tiffPaths.length })}
          </p>
          <ol aria-label={t('import.files.tiffList')} className={styles.files}>
            {tiffPaths.map((path) => {
              const name = fileName(path);
              return (
                <li key={path} className={styles.tiffFile}>
                  <span className={styles.name}>{name}</span>
                  <button
                    type="button"
                    aria-label={t('import.files.tiffRemove', { name })}
                    onClick={() => files.removeTiff(path)}
                  >
                    {t('import.files.tiffRemoveShort')}
                  </button>
                </li>
              );
            })}
          </ol>
        </>
      )}

      {kind === 'camera' && groups.length > 0 && (
        <>
          <p role="status" className={styles.summary}>
            {t('import.files.summary', { count: files.count })}
            {files.attention > 0 &&
              ` ${t('import.files.needAttention', { count: files.attention })}`}
          </p>
          <ol aria-label={t('import.files.candidates')} className={styles.groups}>
            {groups.map((group, index) => {
              const complete = isComplete(group);
              const number = index + 1;
              return (
                <li key={group.id} className={styles.group}>
                  <div className={styles.groupHead}>
                    {complete ? (
                      <label>
                        <input
                          type="checkbox"
                          checked={group.confirmed}
                          onChange={(e) =>
                            update(group.id, (g) => ({ ...g, confirmed: e.target.checked }))
                          }
                        />{' '}
                        {t('import.files.confirm', { n: number })}
                      </label>
                    ) : (
                      <strong>{t('import.files.group', { n: number })}</strong>
                    )}
                  </div>
                  {group.problem && (
                    <p role="alert" className={styles.problem}>
                      {t(`import.files.problem.${group.problem.kind}`, {
                        count:
                          group.problem.kind === 'missing_shots'
                            ? group.problem.missing
                            : group.problem.kind === 'extra_shots'
                              ? group.problem.extra
                              : 0,
                      })}
                    </p>
                  )}
                  <ol aria-label={t('import.files.files', { n: number })} className={styles.files}>
                    {group.files.map((file, i) => {
                      const name = fileName(file.path);
                      return (
                        <li key={file.path} className={styles.file}>
                          <span className={styles.channel}>
                            {complete
                              ? t(`import.channel.${CHANNELS[i] ?? 'red'}`)
                              : t('import.files.shot', { n: i + 1 })}
                          </span>
                          <span className={styles.name}>{name}</span>
                          <time className={styles.time}>{when(file.captured)}</time>
                          {complete && (
                            <span className={styles.moves}>
                              <button
                                type="button"
                                aria-label={t('import.files.earlier', { name })}
                                disabled={i === 0}
                                onClick={() =>
                                  update(group.id, (g) => ({
                                    ...g,
                                    files: moveFile(g.files, i, -1),
                                  }))
                                }
                              >
                                {t('import.files.earlierShort')}
                              </button>
                              <button
                                type="button"
                                aria-label={t('import.files.later', { name })}
                                disabled={i === group.files.length - 1}
                                onClick={() =>
                                  update(group.id, (g) => ({
                                    ...g,
                                    files: moveFile(g.files, i, 1),
                                  }))
                                }
                              >
                                {t('import.files.laterShort')}
                              </button>
                            </span>
                          )}
                        </li>
                      );
                    })}
                  </ol>
                </li>
              );
            })}
          </ol>
        </>
      )}
    </StepSection>
  );
}
