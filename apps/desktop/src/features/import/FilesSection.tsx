import { ProgressList } from '@gtp/ui';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { importFiles, jobCancel, pickCameraFiles, tripletGroup } from '../../shared/ipc';
import { describeJob } from '../library/JobsPanel';
import { messageOf, useLibraryStore } from '../library/library-store';
import styles from './import.module.css';
import { fileName, importPaths, isComplete, moveFile, toGroups, type Group } from './triplets';

const CHANNELS = ['red', 'green', 'blue'] as const;

/**
 * The camera files of a roll: pick NEF files, see them grouped into triplets by capture time, fix
 * the order of each triplet, confirm the ones to import, and start the import job.
 */
export function FilesSection() {
  const { t, i18n } = useTranslation();
  const rolls = useLibraryStore((s) => s.rolls);
  const currentRoll = useLibraryStore((s) => s.rollId);
  const readOnly = useLibraryStore((s) => s.library?.read_only ?? false);
  const allJobs = useLibraryStore((s) => s.jobs);
  const addJob = useLibraryStore((s) => s.addJob);
  const dismissJob = useLibraryStore((s) => s.dismissJob);

  const jobs = allJobs.filter((j) => j.kind === 'import');

  const [chosenRoll, setChosenRoll] = useState<string | null>(null);
  const [groups, setGroups] = useState<Group[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [started, setStarted] = useState(false);

  const rollId = chosenRoll ?? currentRoll ?? rolls[0]?.id ?? null;
  const roll = rolls.find((r) => r.id === rollId) ?? null;
  const paths = importPaths(groups);
  const ready = groups.filter((g) => g.confirmed && isComplete(g)).length;
  const attention = groups.filter((g) => !isComplete(g)).length;

  const when = (iso: string | null | undefined) => {
    if (!iso) return t('import.files.noTime');
    const date = new Date(iso);
    return Number.isNaN(date.getTime())
      ? iso
      : new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'medium' }).format(
          date,
        );
  };

  const choose = async () => {
    setBusy(true);
    setError(null);
    setStarted(false);
    try {
      const picked = await pickCameraFiles();
      if (picked.length === 0) return;
      setGroups(toGroups(await tripletGroup(picked)));
    } catch (e) {
      setError(t('import.files.readFailed', { message: messageOf(e) }));
    } finally {
      setBusy(false);
    }
  };

  const update = (id: number, change: (group: Group) => Group) =>
    setGroups((all) => all.map((g) => (g.id === id ? change(g) : g)));

  const start = async () => {
    if (!roll) return;
    setBusy(true);
    setError(null);
    try {
      const id = await importFiles(paths, roll.id, 'RgbTriplet');
      addJob({ id, kind: 'import', rollName: roll.name, fraction: 0, state: 'running' });
      setGroups([]);
      setStarted(true);
    } catch (e) {
      setError(t('import.files.startFailed', { message: messageOf(e) }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section aria-labelledby="import-files" className={styles.section}>
      <h3 id="import-files" className={styles.heading}>
        {t('import.files.heading')}
      </h3>
      <p className={styles.muted}>{t('import.files.intro')}</p>

      {rolls.length === 0 ? (
        <p role="status">{t('import.files.noRolls')}</p>
      ) : (
        <div className={styles.row}>
          <label className={styles.field}>
            {t('import.files.roll')}
            <select value={rollId ?? ''} onChange={(e) => setChosenRoll(e.target.value)}>
              {rolls.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.name}
                </option>
              ))}
            </select>
          </label>
          <button type="button" disabled={busy || readOnly} onClick={() => void choose()}>
            {busy && groups.length === 0 && !started
              ? t('import.files.choosing')
              : t('import.files.choose')}
          </button>
        </div>
      )}

      {error && <p role="alert">{error}</p>}
      {started && <p role="status">{t('import.files.started')}</p>}

      {groups.length > 0 && (
        <>
          <p role="status" className={styles.summary}>
            {t('import.files.summary', { count: ready })}
            {attention > 0 && ` ${t('import.files.needAttention', { count: attention })}`}
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
          <div className={styles.buttons}>
            <button
              type="button"
              disabled={busy || readOnly || paths.length === 0 || !roll}
              onClick={() => void start()}
            >
              {t('import.files.import', { count: ready })}
            </button>
          </div>
        </>
      )}

      <ProgressList
        items={jobs.map((job) => describeJob(job, t, i18n.language))}
        label={t('jobs.label')}
        cancelLabel={t('jobs.cancel')}
        dismissLabel={t('jobs.dismiss')}
        onCancel={(id) => void jobCancel(id)}
        onDismiss={dismissJob}
      />
    </section>
  );
}
