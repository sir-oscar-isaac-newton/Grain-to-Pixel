/**
 * The keys of the Develop view, by action. They are defaults: customizable shortcuts (U-13) will
 * replace this map with the user's own. `[` and `]` and `\` are the ones the spec names.
 */
export const DEVELOP_KEYS = {
  toggleBefore: '\\',
  previousFrame: '[',
  nextFrame: ']',
  backToLibrary: 'Escape',
  fit: '0',
  actualSize: '1',
  zoomIn: '+',
  zoomOut: '-',
} as const;

export type DevelopAction = keyof typeof DEVELOP_KEYS;

/** The action a key press stands for, or null. `=` counts as `+`, so Shift is not needed. */
export function actionForKey(key: string): DevelopAction | null {
  const wanted = key === '=' ? '+' : key;
  for (const [action, bound] of Object.entries(DEVELOP_KEYS)) {
    if (bound === wanted) return action as DevelopAction;
  }
  return null;
}
