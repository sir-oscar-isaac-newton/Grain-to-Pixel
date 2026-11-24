import { useRef, type ReactNode } from 'react';
import styles from '../../import.module.css';
import { useFocusHeading } from '../../use-focus-heading';

interface StepSectionProps {
  /** The id of the heading, which names the section. */
  id: string;
  heading: string;
  children: ReactNode;
}

/** A page of the camera steps: a section whose heading takes the focus when the page opens. */
export function StepSection({ id, heading, children }: StepSectionProps) {
  const headingRef = useRef<HTMLHeadingElement>(null);
  useFocusHeading(headingRef, true);
  return (
    <section aria-labelledby={id} className={styles.section}>
      <h3 id={id} ref={headingRef} tabIndex={-1} className={styles.heading}>
        {heading}
      </h3>
      {children}
    </section>
  );
}
