import * as Dialog from '@radix-ui/react-dialog';
import { useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { formatBytes } from '../../shared/format';
import { folderCheck, pickFolder, type FolderCheck } from '../../shared/ipc';
import styles from './library.module.css';
import { messageOf, useLibraryStore } from './library-store';

type Mode = 'create' | 'open';

/** Below this much free space a warning is shown (a 36-frame roll takes 2 to 4 GB). */
const LOW_SPACE_BYTES = 20_000_000_000;

const folderName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? '';

/**
 * Shown until a library is open: create one in an empty folder or open an existing one. It shows
 * where the library will live and how much space is left there, and cannot be dismissed.
 */
export function FirstRunDialog() {
  const { t, i18n } = useTranslation();
  const startError = useLibraryStore((s) => s.startError);
  const createLibrary = useLibraryStore((s) => s.createLibrary);
  const openLibrary = useLibraryStore((s) => s.openLibrary);

  const [mode, setMode] = useState<Mode>('create');
  const [path, setPath] = useState<string | null>(null);
  const [check, setCheck] = useState<FolderCheck | null>(null);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const choose = async () => {
    setError(null);
    try {
      const picked = await pickFolder();
      if (!picked) return;
      setPath(picked);
      setCheck(await folderCheck(picked));
      setName((current) => current || folderName(picked));
    } catch (e) {
      setError(messageOf(e));
    }
  };

  const problem =
    check === null
      ? null
      : mode === 'create'
        ? check.has_library
          ? 'firstRun.hasLibrary'
          : check.is_empty
            ? null
            : 'firstRun.notEmpty'
        : check.has_library
          ? null
          : 'firstRun.noLibrary';
  const ready =
    path !== null && check !== null && problem === null && (mode === 'open' || name.trim() !== '');

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!ready || path === null) return;
    setBusy(true);
    setError(null);
    try {
      if (mode === 'create') await createLibrary(path, name.trim());
      else await openLibrary(path);
    } catch (e) {
      setError(messageOf(e));
      setBusy(false);
    }
  };

  return (
    <Dialog.Root open>
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content
          className={styles.dialog}
          onEscapeKeyDown={(e) => e.preventDefault()}
          onPointerDownOutside={(e) => e.preventDefault()}
          onInteractOutside={(e) => e.preventDefault()}
        >
          <Dialog.Title>{t('firstRun.title')}</Dialog.Title>
          <Dialog.Description>{t('firstRun.description')}</Dialog.Description>
          {startError && <p role="alert">{t('firstRun.lastFailed', { message: startError })}</p>}

          <form onSubmit={submit} className={styles.form}>
            <fieldset className={styles.fieldset}>
              <legend>{t('firstRun.mode')}</legend>
              {(['create', 'open'] as const).map((m) => (
                <label key={m} className={styles.radio}>
                  <input
                    type="radio"
                    name="mode"
                    value={m}
                    checked={mode === m}
                    onChange={() => setMode(m)}
                  />
                  {t(`firstRun.${m}`)}
                </label>
              ))}
            </fieldset>

            <div className={styles.folderRow}>
              <span>{t('firstRun.folder')}</span>
              <span className={styles.path}>{path ?? t('firstRun.noFolder')}</span>
              <button type="button" onClick={() => void choose()} disabled={busy}>
                {t('firstRun.choose')}
              </button>
            </div>

            {check && (
              <dl className={styles.facts}>
                <dt>{t('firstRun.location')}</dt>
                <dd>{check.path}</dd>
                <dt>{t('firstRun.free')}</dt>
                <dd>
                  {check.free_bytes === null
                    ? t('firstRun.freeUnknown')
                    : formatBytes(check.free_bytes, i18n.language)}
                </dd>
              </dl>
            )}
            {check && check.free_bytes !== null && check.free_bytes < LOW_SPACE_BYTES && (
              <p role="note">{t('firstRun.lowSpace')}</p>
            )}
            {problem && <p role="alert">{t(problem)}</p>}

            {mode === 'create' && (
              <label className={styles.field}>
                {t('firstRun.name')}
                <input
                  type="text"
                  value={name}
                  maxLength={200}
                  placeholder={t('firstRun.namePlaceholder')}
                  onChange={(e) => setName(e.target.value)}
                />
              </label>
            )}

            {error && <p role="alert">{error}</p>}
            <div className={styles.buttons}>
              <button type="submit" disabled={!ready || busy}>
                {busy
                  ? t('firstRun.working')
                  : mode === 'create'
                    ? t('firstRun.submitCreate')
                    : t('firstRun.submitOpen')}
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
