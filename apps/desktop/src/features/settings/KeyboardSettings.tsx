import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ACTION_IDS,
  COLOUR_ACTIONS,
  DEFAULT_KEYS,
  bindingProblem,
  effectiveKeys,
  keyLabel,
  normalizeKey,
  overridesOf,
  type ActionId,
} from '../../shared/keymap';
import styles from './settings.module.css';

/** Keys that only modify another key: pressing one alone does not choose anything. */
const MODIFIERS = ['Shift', 'Control', 'Alt', 'Meta', 'AltGraph', 'CapsLock'];

interface KeyboardSettingsProps {
  /** The keys the user changed (action name to key), as stored. */
  overrides: Readonly<Record<string, string>>;
  /** Stores all the changed keys; rejects with the reason when that is not possible. */
  onSave: (overrides: Record<string, string>) => Promise<void>;
  /** Nothing can be changed (a save is under way). */
  busy: boolean;
}

/**
 * The keyboard section of Settings: every shortcut with its key, a button to change it (the next
 * key pressed becomes the new one, Escape cancels) and a reset to the defaults. A key that
 * another action has is refused, naming that action.
 */
export function KeyboardSettings({ overrides, onSave, busy }: KeyboardSettingsProps) {
  const { t } = useTranslation();
  const map = effectiveKeys(overrides);
  const [picking, setPicking] = useState<ActionId | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const nameOf = (id: ActionId) => t(`settings.keys.action.${id}`);

  useEffect(() => {
    if (picking === null) return;
    const onKey = (event: KeyboardEvent) => {
      // A modifier on its own is the start of a combination: wait for the key itself.
      if (MODIFIERS.includes(event.key)) return;
      event.preventDefault();
      event.stopPropagation();
      if (event.key === 'Escape') {
        setPicking(null);
        setMessage(null);
        return;
      }
      if (event.ctrlKey || event.altKey || event.metaKey) {
        setMessage(t('settings.keys.modified'));
        return;
      }
      const problem = bindingProblem(map, picking, event.key);
      if (problem?.kind === 'invalid') {
        setMessage(t('settings.keys.invalid', { key: keyLabel(event.key) }));
        return;
      }
      if (problem?.kind === 'taken') {
        setMessage(
          t('settings.keys.taken', { key: keyLabel(event.key), action: nameOf(problem.by) }),
        );
        return;
      }
      const chosen = picking;
      setPicking(null);
      setMessage(null);
      void onSave(overridesOf({ ...map, [chosen]: normalizeKey(event.key) }));
    };
    // Before anything else sees the key: it is being chosen, not used.
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
    // `map` and `nameOf` follow `overrides` and the language, which end a pick anyway.
  }, [picking, overrides, onSave, t]);

  const group = (heading: string, ids: readonly ActionId[], note?: string) => (
    <section aria-labelledby={`keys-${heading}`} className={styles.keyGroup}>
      <h4 id={`keys-${heading}`} className={styles.subheading}>
        {t(`settings.keys.group.${heading}`)}
      </h4>
      {note && <p className={styles.muted}>{note}</p>}
      <ul className={styles.keyList}>
        {ids.map((id) => (
          <li key={id} className={styles.keyRow}>
            <span>{nameOf(id)}</span>
            <kbd className={styles.key}>{keyLabel(map[id])}</kbd>
            <span className={styles.changed}>
              {map[id] !== DEFAULT_KEYS[id] && t('settings.keys.changed')}
            </span>
            <button
              type="button"
              aria-pressed={picking === id}
              aria-label={t('settings.keys.changeFor', { action: nameOf(id) })}
              disabled={busy}
              onClick={() => {
                setMessage(null);
                setPicking(picking === id ? null : id);
              }}
            >
              {t('settings.keys.change')}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );

  const colour = ACTION_IDS.filter((id) => COLOUR_ACTIONS[id]);
  const develop = ACTION_IDS.filter((id) => !COLOUR_ACTIONS[id]);
  const differs = Object.keys(overridesOf(map)).length > 0;

  return (
    <section aria-labelledby="settings-keys" className={styles.section}>
      <h3 id="settings-keys" className={styles.heading}>
        {t('settings.keys.heading')}
      </h3>
      <p className={styles.muted}>{t('settings.keys.intro')}</p>
      {group('develop', develop)}
      {group('colour', colour, t('settings.keys.colourNote'))}
      {picking !== null && (
        <p role="status">{t('settings.keys.press', { action: nameOf(picking) })}</p>
      )}
      {message && <p role="alert">{message}</p>}
      <div className={styles.buttons}>
        <button type="button" disabled={!differs || busy} onClick={() => void onSave({})}>
          {t('settings.keys.reset')}
        </button>
      </div>
    </section>
  );
}
