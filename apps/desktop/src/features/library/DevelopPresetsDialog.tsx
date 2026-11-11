import * as Dialog from '@radix-ui/react-dialog';
import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { settingsGet, settingsSet, type DevelopPreset, type Recipe } from '../../shared/ipc';
import {
  DEFAULT_PRESET_PARTS,
  PRESET_PARTS,
  orderedParts,
  presetNameProblem,
} from './develop-presets';
import styles from './library.module.css';
import { messageOf } from './library-store';

interface DevelopPresetsDialogProps {
  /**
   * `manage` (the Develop view) lists, saves, updates, renames, deletes and applies presets;
   * `apply` (the Library) only lists them and applies one.
   */
  mode: 'manage' | 'apply';
  onClose: () => void;
  /** Applies a preset and returns what to tell the user. A refusal is shown in the dialog. */
  onApply: (preset: DevelopPreset) => Promise<string>;
  /**
   * Whether applying is possible at all (not in a read-only library). Presets are app settings, so
   * saving, updating, renaming and deleting do not depend on it.
   */
  canApply: boolean;
  /** The edit a preset is saved from or updated with (manage mode); null while there is none. */
  current?: Recipe | null;
}

type Editing = { kind: 'rename' | 'delete'; preset: DevelopPreset };

/**
 * The Develop presets: named groups of settings kept in the app settings. In the Develop view they
 * are managed and applied to the open edit; in the Library one is applied to the selected frames.
 */
export function DevelopPresetsDialog({
  mode,
  onClose,
  onApply,
  canApply,
  current = null,
}: DevelopPresetsDialogProps) {
  const { t, i18n } = useTranslation();
  const manage = mode === 'manage';
  const [presets, setPresets] = useState<DevelopPreset[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  // The name being typed, for a new preset or a rename; null when the field is not shown.
  const [naming, setNaming] = useState<string | null>(null);
  const [parts, setParts] = useState<readonly string[]>(DEFAULT_PRESET_PARTS);
  const nameId = useId();
  const nameErrorId = useId();
  const nameRef = useRef<HTMLInputElement>(null);
  const nameShown = naming !== null;

  // The presets come from the settings when the dialog opens.
  useEffect(() => {
    let alive = true;
    settingsGet().then(
      (settings) => {
        if (!alive) return;
        setPresets(settings.develop_presets ?? []);
        setLoaded(true);
      },
      (e: unknown) => {
        if (alive) setError(t('develop.presets.loadFailed', { message: messageOf(e) }));
      },
    );
    return () => {
      alive = false;
    };
  }, [t]);

  useEffect(() => {
    if (nameShown) nameRef.current?.focus();
  }, [nameShown, editing]);

  const list = (items: string[]) =>
    new Intl.ListFormat(i18n.language, { style: 'long', type: 'conjunction' }).format(items);
  const partsLine = (preset: DevelopPreset) =>
    list(orderedParts(preset.fields).map((part) => t(`apply.short.${part}`)));

  /** Writes the whole list; true when it was saved (the list shown is then the saved one). */
  const write = async (next: DevelopPreset[]): Promise<boolean> => {
    setError(null);
    setBusy(true);
    try {
      const saved = await settingsSet({ develop_presets: next });
      setPresets(saved.develop_presets ?? next);
      return true;
    } catch (e) {
      setError(t('develop.presets.failed', { message: messageOf(e) }));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const stopNaming = () => {
    setNaming(null);
    setEditing(null);
  };

  const typedName = (naming ?? '').trim();
  const renaming = editing?.kind === 'rename' ? editing.preset : undefined;
  const problem = nameShown ? presetNameProblem(typedName, presets, renaming) : null;
  const savable = problem === null && (renaming !== undefined || parts.length > 0);
  // An empty name only disables Save; the message is for a name that is typed but not usable.
  const problemShown = problem !== null && naming !== '';

  const save = async (event?: FormEvent) => {
    event?.preventDefault();
    if (!nameShown || !savable || busy) return;
    if (renaming) {
      const next = presets.map((p) => (p === renaming ? { ...p, name: typedName } : p));
      if (await write(next)) stopNaming();
      return;
    }
    if (!current) return;
    const fields = PRESET_PARTS.filter((part) => parts.includes(part));
    if (await write([...presets, { name: typedName, fields, recipe: current }])) {
      stopNaming();
      setParts(DEFAULT_PRESET_PARTS);
    }
  };

  const update = async (preset: DevelopPreset) => {
    if (!current) return;
    await write(presets.map((p) => (p === preset ? { ...p, recipe: current } : p)));
  };

  const remove = async (preset: DevelopPreset) => {
    if (await write(presets.filter((p) => p !== preset))) setEditing(null);
  };

  const apply = async (preset: DevelopPreset) => {
    if (busy || !canApply) return;
    setBusy(true);
    setError(null);
    try {
      setDone(await onApply(preset));
    } catch (e) {
      setError(t('develop.presets.applyFailed', { message: messageOf(e) }));
    } finally {
      setBusy(false);
    }
  };

  const toggle = (part: string) =>
    setParts((now) => (now.includes(part) ? now.filter((p) => p !== part) : [...now, part]));

  const nameField = (
    <div className={styles.field}>
      <label htmlFor={nameId}>{t('develop.presets.name')}</label>
      <input
        id={nameId}
        type="text"
        value={naming ?? ''}
        maxLength={100}
        ref={nameRef}
        aria-invalid={problemShown}
        aria-describedby={problemShown ? nameErrorId : undefined}
        onChange={(e) => setNaming(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            void save();
          }
        }}
      />
      {problemShown && (
        <p id={nameErrorId} role="alert">
          {t(problem)}
        </p>
      )}
    </div>
  );

  return (
    <Dialog.Root open onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content className={styles.dialog}>
          <Dialog.Title>
            {manage ? t('develop.presets.title') : t('library.applyPreset.title')}
          </Dialog.Title>
          <Dialog.Description>
            {manage ? t('develop.presets.description') : t('library.applyPreset.description')}
          </Dialog.Description>

          {done !== null ? (
            <div className={styles.stack}>
              <p role="status">{done}</p>
              <div className={styles.buttons}>
                <button type="button" onClick={onClose}>
                  {t('develop.presets.close')}
                </button>
              </div>
            </div>
          ) : (
            <div className={styles.stack}>
              {loaded && presets.length === 0 && (
                <p className={styles.hint}>
                  {manage ? t('develop.presets.empty') : t('library.applyPreset.empty')}
                </p>
              )}
              {!canApply && manage && <p className={styles.hint}>{t('develop.presets.noApply')}</p>}
              {presets.length > 0 && (
                <ul className={styles.presetList} aria-label={t('develop.presets.list')}>
                  {presets.map((preset) => (
                    <li key={preset.name} className={styles.presetItem}>
                      <div className={styles.field}>
                        <strong>{preset.name}</strong>
                        <span className={styles.hint}>{partsLine(preset)}</span>
                      </div>
                      <div className={styles.buttons}>
                        <button
                          type="button"
                          disabled={!canApply || busy}
                          aria-label={t('develop.presets.applyLabel', { name: preset.name })}
                          onClick={() => void apply(preset)}
                        >
                          {t('develop.presets.apply')}
                        </button>
                        {manage && (
                          <>
                            <button
                              type="button"
                              disabled={!current || busy}
                              aria-label={t('develop.presets.updateLabel', { name: preset.name })}
                              onClick={() => void update(preset)}
                            >
                              {t('develop.presets.update')}
                            </button>
                            <button
                              type="button"
                              disabled={busy}
                              aria-label={t('develop.presets.renameLabel', { name: preset.name })}
                              onClick={() => {
                                setEditing({ kind: 'rename', preset });
                                setNaming(preset.name);
                              }}
                            >
                              {t('develop.presets.rename')}
                            </button>
                            <button
                              type="button"
                              disabled={busy}
                              aria-label={t('develop.presets.deleteLabel', { name: preset.name })}
                              onClick={() => {
                                setNaming(null);
                                setEditing({ kind: 'delete', preset });
                              }}
                            >
                              {t('develop.presets.delete')}
                            </button>
                          </>
                        )}
                      </div>
                    </li>
                  ))}
                </ul>
              )}

              {manage && renaming && (
                <form className={styles.form} onSubmit={save}>
                  {nameField}
                  <div className={styles.buttons}>
                    <button type="submit" disabled={!savable || busy}>
                      {t('develop.presets.renameSave')}
                    </button>
                    <button type="button" onClick={stopNaming}>
                      {t('develop.presets.cancel')}
                    </button>
                  </div>
                </form>
              )}

              {manage && editing?.kind === 'delete' && (
                <div role="group" aria-label={t('develop.presets.confirmLabel')}>
                  <p>{t('develop.presets.confirmDelete', { name: editing.preset.name })}</p>
                  <div className={styles.buttons}>
                    <button type="button" onClick={() => void remove(editing.preset)}>
                      {t('develop.presets.confirm')}
                    </button>
                    <button type="button" onClick={() => setEditing(null)}>
                      {t('develop.presets.keep')}
                    </button>
                  </div>
                </div>
              )}

              {manage && !renaming && !nameShown && (
                <div className={styles.buttons}>
                  <button type="button" disabled={!current || busy} onClick={() => setNaming('')}>
                    {t('develop.presets.saveAs')}
                  </button>
                </div>
              )}

              {manage && !renaming && nameShown && (
                <form className={styles.form} onSubmit={save}>
                  {nameField}
                  <fieldset className={styles.fieldset}>
                    <legend>{t('develop.presets.parts')}</legend>
                    {PRESET_PARTS.map((part) => (
                      <label key={part} className={styles.radio}>
                        <input
                          type="checkbox"
                          checked={parts.includes(part)}
                          onChange={() => toggle(part)}
                        />
                        {t(`apply.group.${part}`)}
                      </label>
                    ))}
                  </fieldset>
                  {parts.length === 0 && (
                    <p className={styles.hint}>{t('develop.presets.noParts')}</p>
                  )}
                  <div className={styles.buttons}>
                    <button type="submit" disabled={!savable || busy}>
                      {t('develop.presets.save')}
                    </button>
                    <button type="button" onClick={stopNaming}>
                      {t('develop.presets.cancel')}
                    </button>
                  </div>
                </form>
              )}

              {error && <p role="alert">{error}</p>}
              <div className={styles.buttons}>
                <button type="button" onClick={onClose}>
                  {t('develop.presets.close')}
                </button>
              </div>
            </div>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
