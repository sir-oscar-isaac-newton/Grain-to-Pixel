import { useEffect, type RefObject } from 'react';

/** Moves focus to the heading when the page mounts, for the pages of the Import guide. */
export function useFocusHeading(ref: RefObject<HTMLElement | null>, enabled: boolean): void {
  useEffect(() => {
    if (enabled) ref.current?.focus();
  }, [ref, enabled]);
}
