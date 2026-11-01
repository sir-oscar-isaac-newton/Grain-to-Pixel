import * as Dialog from '@radix-ui/react-dialog';
import { useEffect, useMemo, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { frameGet, frameList, recipeApplyTo, type Frame, type RecipeField } from '../../shared/ipc';
import styles from './library.module.css';
import { messageOf, useLibraryStore } from './library-store';

/** The groups of settings that can be copied, in the order they are shown. */
const GROUPS: readonly RecipeField[] = [
  'Color',
  'Tone',
  'Geometry',
  'Base',
  'Invert',
  'Dust',
  'Output',
];
/** Ticked when the dialog opens: crop, base and levels usually differ from frame to frame. */
const DEFAULT_GROUPS: readonly RecipeField[] = ['Color', 'Tone'];
/** Frames offered as the source when many are selected (the rest can still be targets). */
export const MAX_SOURCES = 50;
const PAGE_SIZE = 500;
/** A roll is read in pages of {@link PAGE_SIZE}; no real roll needs more than a few. */
const MAX_PAGES = 20;

type Target = 'selection' | 'roll';

interface ApplySettingsDialogProps {
  open: boolean;
  onClose: () => void;
  /** The frames the settings can come from: the open frame, or the selected frames. */
  sources: readonly string[];
  /** The frames selected in the Library, which can receive the settings. */
  selected: readonly string[];
}

/**
 * Copies chosen groups of settings from one frame to the other selected frames or to the whole
 * roll of the source, after saying what will be replaced. The library's own `library://changed`
 * refreshes the thumbnails.
 */
export function ApplySettingsDialog({
  open,
  onClose,
  sources,
  selected,
}: ApplySettingsDialogProps) {
  const { t, i18n } = useTranslation();
  const rolls = useLibraryStore((s) => s.rolls);

  const offered = useMemo(() => sources.slice(0, MAX_SOURCES), [sources]);
  const [sourceId, setSourceId] = useState<string>(offered[0] ?? '');
  const [info, setInfo] = useState<Record<string, Frame>>({});
  const [rollFrames, setRollFrames] = useState<Record<string, string[]>>({});
  const [groups, setGroups] = useState<readonly RecipeField[]>(DEFAULT_GROUPS);
  const [target, setTarget] = useState<Target | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [applied, setApplied] = useState<number | null>(null);

  // The source starts as the first frame offered, and is kept while it is still offered.
  useEffect(() => {
    setSourceId((current) => (offered.includes(current) ? current : (offered[0] ?? '')));
  }, [offered]);

  // What the frames offered are called (their number and roll).
  useEffect(() => {
    if (!open) return;
    let alive = true;
    void Promise.all(
      offered.map((id) =>
        frameGet(id).then(
          (frame) => frame,
          () => null,
        ),
      ),
    ).then((frames) => {
      if (!alive) return;
      setInfo(
        Object.fromEntries(frames.flatMap((frame) => (frame ? [[frame.id, frame] as const] : []))),
      );
    });
    return () => {
      alive = false;
    };
  }, [open, offered]);

  const source = info[sourceId];
  const rollId = source?.roll_id;

  // The frames of the source's roll, for the "whole roll" target.
  useEffect(() => {
    if (!open || !rollId || rollFrames[rollId]) return;
    let alive = true;
    void (async () => {
      const ids: string[] = [];
      for (let page = 0; page < MAX_PAGES; page += 1) {
        const result = await frameList({ roll_id: rollId, page, page_size: PAGE_SIZE });
        ids.push(...result.items.map((frame) => frame.id));
        if (ids.length >= result.total || result.items.length === 0) break;
      }
      if (alive) setRollFrames((known) => ({ ...known, [rollId]: ids }));
    })().catch(() => {
      // The roll option stays unavailable; applying to the selection still works.
    });
    return () => {
      alive = false;
    };
  }, [open, rollId, rollFrames]);

  const others = (ids: readonly string[]) => [...new Set(ids)].filter((id) => id !== sourceId);
  const selectionTargets = others(selected);
  const rollTargets = rollId && rollFrames[rollId] ? others(rollFrames[rollId]) : null;

  // The target defaults to the selection when there is one, else to the roll.
  const chosen: Target | null =
    target === 'selection' && selectionTargets.length > 0
      ? 'selection'
      : target === 'roll' && rollTargets && rollTargets.length > 0
        ? 'roll'
        : selectionTargets.length > 0
          ? 'selection'
          : rollTargets && rollTargets.length > 0
            ? 'roll'
            : null;
  const targets =
    chosen === 'selection' ? selectionTargets : chosen === 'roll' ? rollTargets : null;

  const rollName = (id: string | undefined) => rolls.find((r) => r.id === id)?.name ?? '';
  const frameName = (frame: Frame | undefined) =>
    frame
      ? t('apply.frameOf', { n: frame.index_in_roll, roll: rollName(frame.roll_id) })
      : t('apply.unknownFrame');
  const list = (items: string[]) =>
    new Intl.ListFormat(i18n.language, { style: 'long', type: 'conjunction' }).format(items);
  const chosenNames = list(
    GROUPS.filter((g) => groups.includes(g)).map((g) => t(`apply.short.${g}`)),
  );

  const toggle = (group: RecipeField) =>
    setGroups((current) =>
      current.includes(group) ? current.filter((g) => g !== group) : [...current, group],
    );

  const ready = sourceId !== '' && groups.length > 0 && targets !== null && targets.length > 0;

  const apply = async (event: FormEvent) => {
    event.preventDefault();
    if (!ready || targets === null || busy) return;
    setBusy(true);
    setError(null);
    try {
      await recipeApplyTo(
        targets,
        sourceId,
        GROUPS.filter((g) => groups.includes(g)),
      );
      setApplied(targets.length);
    } catch (e) {
      setError(t('apply.failed', { message: messageOf(e) }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content className={styles.dialog}>
          <Dialog.Title>{t('apply.title')}</Dialog.Title>
          <Dialog.Description>{t('apply.description')}</Dialog.Description>

          {applied !== null ? (
            <div className={styles.stack}>
              <p role="status">{t('apply.done', { count: applied })}</p>
              <div className={styles.buttons}>
                <button type="button" onClick={onClose}>
                  {t('apply.close')}
                </button>
              </div>
            </div>
          ) : (
            <form onSubmit={apply} className={styles.form}>
              {offered.length > 1 ? (
                <label className={styles.field}>
                  {t('apply.from')}
                  <select value={sourceId} onChange={(e) => setSourceId(e.target.value)}>
                    {offered.map((id) => (
                      <option key={id} value={id}>
                        {frameName(info[id])}
                      </option>
                    ))}
                  </select>
                </label>
              ) : (
                <p>{t('apply.fromOne', { frame: frameName(source) })}</p>
              )}

              <fieldset className={styles.fieldset}>
                <legend>{t('apply.groups')}</legend>
                {GROUPS.map((group) => (
                  <label key={group} className={styles.radio}>
                    <input
                      type="checkbox"
                      checked={groups.includes(group)}
                      onChange={() => toggle(group)}
                    />
                    {t(`apply.group.${group}`)}
                    <span className={styles.hint}>{t(`apply.hint.${group}`)}</span>
                  </label>
                ))}
              </fieldset>

              <fieldset className={styles.fieldset}>
                <legend>{t('apply.to')}</legend>
                <label className={styles.radio}>
                  <input
                    type="radio"
                    name="target"
                    checked={chosen === 'selection'}
                    disabled={selectionTargets.length === 0}
                    onChange={() => setTarget('selection')}
                  />
                  {t('apply.selection', { count: selectionTargets.length })}
                </label>
                <label className={styles.radio}>
                  <input
                    type="radio"
                    name="target"
                    checked={chosen === 'roll'}
                    disabled={!rollTargets || rollTargets.length === 0}
                    onChange={() => setTarget('roll')}
                  />
                  {rollTargets
                    ? t('apply.roll', { count: rollTargets.length, roll: rollName(rollId) })
                    : t('apply.rollLoading')}
                </label>
              </fieldset>

              {ready && targets && (
                <p className={styles.hint}>
                  {t('apply.confirm', { groups: chosenNames, count: targets.length })}
                </p>
              )}
              {groups.length === 0 && <p className={styles.hint}>{t('apply.noGroups')}</p>}
              {error && <p role="alert">{error}</p>}
              <div className={styles.buttons}>
                <button type="submit" disabled={!ready || busy}>
                  {busy
                    ? t('apply.applying')
                    : ready && targets
                      ? t('apply.apply', { groups: chosenNames, count: targets.length })
                      : t('apply.applyNone')}
                </button>
                <button type="button" onClick={onClose}>
                  {t('apply.cancel')}
                </button>
              </div>
            </form>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
