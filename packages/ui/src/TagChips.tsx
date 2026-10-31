import { useId, useState } from 'react';
import styles from './TagChips.module.css';

/** The glyph on the remove button; its meaning is in the button's label. */
const CROSS = '×';

export interface TagChip {
  id: string;
  name: string;
}

interface TagChipsProps {
  /** Names the group for assistive technology, for example "Tags". */
  label: string;
  tags: readonly TagChip[];
  /** Shown instead of the chips when there are none. */
  emptyText?: string;
  /** Present: each chip gets a remove button named by this function. */
  removeLabel?: (name: string) => string;
  onRemove?: (tag: TagChip) => void;
  /** Present: an input to add a tag by name (Enter or the button). */
  onAdd?: (name: string) => void;
  addLabel?: string;
  addButtonLabel?: string;
  /** Names offered while typing in the add input. */
  suggestions?: readonly string[];
  disabled?: boolean;
}

/**
 * Tags as chips, optionally removable, with an optional input to add one. It holds no text of its
 * own: every label comes from the caller, who also decides what adding and removing do.
 */
export function TagChips({
  label,
  tags,
  emptyText,
  removeLabel,
  onRemove,
  onAdd,
  addLabel,
  addButtonLabel,
  suggestions = [],
  disabled = false,
}: TagChipsProps) {
  const listId = useId();
  const [draft, setDraft] = useState('');

  const submit = () => {
    const name = draft.trim();
    if (name === '' || !onAdd) return;
    onAdd(name);
    setDraft('');
  };

  return (
    <div role="group" aria-label={label} className={styles.root}>
      {tags.length === 0 ? (
        emptyText && <span className={styles.empty}>{emptyText}</span>
      ) : (
        <ul className={styles.chips}>
          {tags.map((tag) => (
            <li key={tag.id} className={styles.chip}>
              <span>{tag.name}</span>
              {onRemove && removeLabel && (
                <button
                  type="button"
                  className={styles.remove}
                  aria-label={removeLabel(tag.name)}
                  disabled={disabled}
                  onClick={() => onRemove(tag)}
                >
                  <span aria-hidden="true">{CROSS}</span>
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {onAdd && (
        // Not a <form>: this sits inside forms (the roll editor), which cannot nest.
        <div className={styles.add}>
          <input
            type="text"
            list={listId}
            aria-label={addLabel}
            value={draft}
            disabled={disabled}
            maxLength={64}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key !== 'Enter') return;
              e.preventDefault();
              submit();
            }}
          />
          <datalist id={listId}>
            {suggestions.map((name) => (
              <option key={name} value={name} />
            ))}
          </datalist>
          <button type="button" disabled={disabled || draft.trim() === ''} onClick={submit}>
            {addButtonLabel}
          </button>
        </div>
      )}
    </div>
  );
}
