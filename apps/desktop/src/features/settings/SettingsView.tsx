import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useUiStore } from '../../app/store';
import { formatBytes } from '../../shared/format';
import {
  libraryInfo,
  settingsDefaults,
  settingsGet,
  settingsSet,
  type LibraryDetails,
} from '../../shared/ipc';
import { messageOf, useLibraryStore } from '../library/library-store';
import { FLAG_NAMES, flagsOf, type FlagName, type FlagValues } from './flags';
import styles from './settings.module.css';

/**
 * Settings: where the library is and how much room is left, and the runtime feature flags with
 * their defaults. A flag is saved the moment it is switched; if saving fails it switches back.
 */
export function SettingsView() {
  const { t, i18n } = useTranslation();
  const setView = useUiStore((s) => s.setView);
  const opened = useLibraryStore((s) => s.library);

  const [library, setLibrary] = useState<LibraryDetails | null>(opened);
  const [flags, setFlags] = useState<FlagValues | null>(null);
  const [defaults, setDefaults] = useState<FlagValues | null>(null);
  const [saving, setSaving] = useState<ReadonlySet<FlagName>>(new Set());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    Promise.all([settingsGet(), settingsDefaults()]).then(
      ([current, initial]) => {
        if (!alive) return;
        setFlags(flagsOf(current));
        setDefaults(flagsOf(initial));
      },
      (e: unknown) => {
        if (alive) setError(t('settings.loadFailed', { message: messageOf(e) }));
      },
    );
    // The free space changes while the app is open: ask again rather than show the figure from
    // when the library was opened.
    libraryInfo().then(
      (info) => {
        if (alive && info.library) setLibrary(info.library);
      },
      () => {
        // The figure from when the library was opened stays.
      },
    );
    return () => {
      alive = false;
    };
  }, [t]);

  const save = async (patch: Partial<FlagValues>) => {
    const names = Object.keys(patch) as FlagName[];
    const before = flags;
    setError(null);
    setSaving((s) => new Set([...s, ...names]));
    setFlags((s) => (s ? { ...s, ...patch } : s));
    try {
      setFlags(flagsOf(await settingsSet({ flags: patch })));
    } catch (e) {
      setFlags(before);
      setError(t('settings.saveFailed', { message: messageOf(e) }));
    } finally {
      setSaving((s) => new Set([...s].filter((n) => !names.includes(n))));
    }
  };

  const differing: FlagName[] =
    flags && defaults ? FLAG_NAMES.filter((name) => flags[name] !== defaults[name]) : [];

  return (
    <section aria-labelledby="settings-title" className={styles.view}>
      <h2 id="settings-title" className={styles.title}>
        {t('settings.title')}
      </h2>

      <section aria-labelledby="settings-library" className={styles.section}>
        <h3 id="settings-library" className={styles.heading}>
          {t('settings.library.heading')}
        </h3>
        {library ? (
          <>
            <dl className={styles.facts}>
              <dt>{t('settings.library.name')}</dt>
              <dd>{library.name}</dd>
              <dt>{t('settings.library.folder')}</dt>
              <dd>{library.path}</dd>
              <dt>{t('settings.library.free')}</dt>
              <dd>
                {typeof library.free_bytes === 'number'
                  ? formatBytes(library.free_bytes, i18n.language)
                  : t('settings.library.freeUnknown')}
              </dd>
            </dl>
            {library.read_only && (
              <p role="status" className={styles.muted}>
                {t('settings.library.readOnly')}
              </p>
            )}
          </>
        ) : (
          <>
            <p className={styles.muted}>{t('settings.library.none')}</p>
            <div className={styles.buttons}>
              <button type="button" onClick={() => setView('library')}>
                {t('settings.library.toLibrary')}
              </button>
            </div>
          </>
        )}
      </section>

      <section aria-labelledby="settings-flags" className={styles.section}>
        <h3 id="settings-flags" className={styles.heading}>
          {t('settings.flags.heading')}
        </h3>
        <p className={styles.muted}>{t('settings.flags.intro')}</p>
        {flags && defaults ? (
          <fieldset className={styles.flags}>
            <legend>{t('settings.flags.legend')}</legend>
            {FLAG_NAMES.map((name) => (
              <label key={name} className={styles.flag}>
                <input
                  type="checkbox"
                  role="switch"
                  checked={flags[name]}
                  disabled={saving.has(name)}
                  onChange={(e) => void save({ [name]: e.target.checked })}
                />
                <span className={styles.flagName}>
                  {t(`settings.flags.${name}.name`)}
                  {flags[name] !== defaults[name] && (
                    <span className={styles.changed}>{t('settings.flags.changed')}</span>
                  )}
                </span>
                <span className={`${styles.muted} ${styles.flagInfo}`}>
                  {t(`settings.flags.${name}.purpose`)}{' '}
                  {defaults[name] ? t('settings.flags.defaultOn') : t('settings.flags.defaultOff')}
                </span>
              </label>
            ))}
          </fieldset>
        ) : (
          !error && <p role="status">{t('settings.loading')}</p>
        )}
        <div className={styles.buttons}>
          <button
            type="button"
            disabled={differing.length === 0 || saving.size > 0}
            onClick={() =>
              defaults &&
              void save(Object.fromEntries(differing.map((name) => [name, defaults[name]])))
            }
          >
            {t('settings.flags.reset')}
          </button>
        </div>
        {error && <p role="alert">{error}</p>}
      </section>
    </section>
  );
}
