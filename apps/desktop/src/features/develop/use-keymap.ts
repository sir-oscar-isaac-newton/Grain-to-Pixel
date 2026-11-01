import { useEffect, useState } from 'react';
import { settingsGet } from '../../shared/ipc';
import { DEFAULT_KEYS, effectiveKeys, type KeyMap } from '../../shared/keymap';

/**
 * The keys in use: the defaults until the settings have been read, then the user's own choices.
 * Read once when the view opens; changing a key in Settings takes effect the next time it does.
 */
export function useKeyMap(): KeyMap {
  const [map, setMap] = useState<KeyMap>(DEFAULT_KEYS);
  useEffect(() => {
    let alive = true;
    settingsGet().then(
      (settings) => {
        if (alive) setMap(effectiveKeys(settings.keys ?? undefined));
      },
      () => {
        // The defaults stay: a settings file that cannot be read must not take the keys away.
      },
    );
    return () => {
      alive = false;
    };
  }, []);
  return map;
}
