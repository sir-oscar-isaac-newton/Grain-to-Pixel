import * as RadixSlider from '@radix-ui/react-slider';
import { useId, useRef } from 'react';
import styles from './Slider.module.css';

interface SliderProps {
  /** Names the slider, for example "Black point, red". */
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  /**
   * Called with the new value. `dragging` is true while the thumb is being moved and false when it
   * is let go (a key press is a release at once, with no move before it), so a screen can show a
   * rough result while dragging and the sharp one on release.
   */
  onChange: (value: number, dragging: boolean) => void;
  /** How a value is written, for the number beside the slider and for assistive technology. */
  format?: (value: number) => string;
  disabled?: boolean;
}

/** A labelled slider with its value beside it (Radix: keyboard, pointer and touch). */
export function Slider({
  label,
  value,
  min,
  max,
  step,
  onChange,
  format = String,
  disabled = false,
}: SliderProps) {
  const id = useId();
  const text = format(value);
  // For a key press Radix reports the release before the move, with the same value. Taking the
  // move after it as "still dragging" would leave a rough result on screen, so it is ignored.
  const released = useRef<number | null>(null);
  return (
    <div className={styles.slider}>
      <label id={id} className={styles.label}>
        {label}
      </label>
      <span className={styles.value} aria-hidden="true">
        {text}
      </span>
      <RadixSlider.Root
        className={styles.root}
        min={min}
        max={max}
        step={step}
        value={[value]}
        disabled={disabled}
        onValueChange={([next]) => {
          if (next === undefined || next === released.current) return;
          onChange(next, true);
        }}
        onValueCommit={([next]) => {
          if (next === undefined) return;
          released.current = next;
          onChange(next, false);
        }}
      >
        <RadixSlider.Track className={styles.track}>
          <RadixSlider.Range className={styles.range} />
        </RadixSlider.Track>
        <RadixSlider.Thumb className={styles.thumb} aria-labelledby={id} aria-valuetext={text} />
      </RadixSlider.Root>
    </div>
  );
}
