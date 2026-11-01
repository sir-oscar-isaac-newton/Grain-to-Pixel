// The keyboard shortcuts (spec section 13): each action has a default key, the user can change any
// of them, and no two actions may share a key. The app stores only the keys the user changed
// (action name to key); this module knows the actions, their defaults and the rules.

/** The actions that have a key, in the order they are listed. */
export const ACTION_IDS = [
  'develop.toggleBefore',
  'develop.previousFrame',
  'develop.nextFrame',
  'develop.backToLibrary',
  'develop.fit',
  'develop.actualSize',
  'develop.zoomIn',
  'develop.zoomOut',
  'colour.red',
  'colour.cyan',
  'colour.green',
  'colour.magenta',
  'colour.blue',
  'colour.yellow',
] as const;

export type ActionId = (typeof ACTION_IDS)[number];
export type KeyMap = Record<ActionId, string>;

/** `\`, `[` and `]` are the keys the spec names for the Develop view. */
export const DEFAULT_KEYS: KeyMap = {
  'develop.toggleBefore': '\\',
  'develop.previousFrame': '[',
  'develop.nextFrame': ']',
  'develop.backToLibrary': 'Escape',
  'develop.fit': '0',
  'develop.actualSize': '1',
  'develop.zoomIn': '+',
  'develop.zoomOut': '-',
  'colour.red': 'q',
  'colour.cyan': 'a',
  'colour.green': 'w',
  'colour.magenta': 's',
  'colour.blue': 'e',
  'colour.yellow': 'd',
};

export type ColourAxis = 'red_cyan' | 'green_magenta' | 'blue_yellow';

/** The colour slider each colour key moves, and which way (positive adds red, green or blue). */
export const COLOUR_ACTIONS: Partial<Record<ActionId, { axis: ColourAxis; sign: 1 | -1 }>> = {
  'colour.red': { axis: 'red_cyan', sign: 1 },
  'colour.cyan': { axis: 'red_cyan', sign: -1 },
  'colour.green': { axis: 'green_magenta', sign: 1 },
  'colour.magenta': { axis: 'green_magenta', sign: -1 },
  'colour.blue': { axis: 'blue_yellow', sign: 1 },
  'colour.yellow': { axis: 'blue_yellow', sign: -1 },
};

/** A colour key press moves its slider this many steps, or one step with Shift. */
export const COLOUR_STEP = 10;
export const COLOUR_FINE_STEP = 1;

const isAction = (id: string): id is ActionId => (ACTION_IDS as readonly string[]).includes(id);

/** A key as compared: letters without case, so Shift never changes which action a key means. */
export function normalizeKey(key: string): string {
  return [...key].length === 1 ? key.toLowerCase() : key;
}

/**
 * Whether a key can be given to an action: one printable character that is not a space (Space
 * presses buttons), or Escape, which is the default of one action and cannot be picked again
 * because it cancels the picking.
 */
export function isValidKey(key: string): boolean {
  if (key === 'Escape') return true;
  return [...key].length === 1 && !/[\s\p{C}]/u.test(key);
}

/**
 * The key of every action: the default, or the user's choice. A choice that is not a valid key,
 * names no action, or would give two actions one key is ignored (the action keeps its default),
 * so a settings file edited by hand cannot leave the view without keys.
 */
export function effectiveKeys(overrides: Readonly<Record<string, string>> | undefined): KeyMap {
  const chosen = new Map<ActionId, string>();
  for (const [id, key] of Object.entries(overrides ?? {})) {
    if (isAction(id) && isValidKey(key) && key !== DEFAULT_KEYS[id]) chosen.set(id, key);
  }
  for (;;) {
    const map: KeyMap = { ...DEFAULT_KEYS };
    for (const [id, key] of chosen) map[id] = key;
    const owners = new Map<string, ActionId[]>();
    for (const id of ACTION_IDS) {
      const key = normalizeKey(map[id]);
      owners.set(key, [...(owners.get(key) ?? []), id]);
    }
    const clashing = [...owners.values()].filter((ids) => ids.length > 1).flat();
    const dropped = clashing.filter((id) => chosen.delete(id));
    if (dropped.length === 0) return map;
  }
}

/** Only the keys that differ from the defaults, which is what is stored. */
export function overridesOf(map: KeyMap): Record<string, string> {
  return Object.fromEntries(
    ACTION_IDS.filter((id) => map[id] !== DEFAULT_KEYS[id]).map((id) => [id, map[id]]),
  );
}

/**
 * The action a pressed key stands for, or null. `=` counts as `+` unless it has a binding of its
 * own, so Shift is not needed to zoom in.
 */
export function actionForKey(map: KeyMap, pressed: string): ActionId | null {
  const wanted = normalizeKey(pressed);
  const find = (key: string) => ACTION_IDS.find((id) => normalizeKey(map[id]) === key) ?? null;
  return find(wanted) ?? (wanted === '=' ? find('+') : null);
}

export type BindingProblem = { kind: 'invalid' } | { kind: 'taken'; by: ActionId };

/** Why `key` cannot be given to `action`, or null when it can. */
export function bindingProblem(map: KeyMap, action: ActionId, key: string): BindingProblem | null {
  if (!isValidKey(key) || key === 'Escape') return { kind: 'invalid' };
  const owner = ACTION_IDS.find(
    (id) => id !== action && normalizeKey(map[id]) === normalizeKey(key),
  );
  return owner ? { kind: 'taken', by: owner } : null;
}

/** A key as people read it. */
export function keyLabel(key: string): string {
  if (key === 'Escape') return 'Esc';
  return [...key].length === 1 ? key.toUpperCase() : key;
}
