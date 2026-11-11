import * as RadixSlider from '@radix-ui/react-slider';
import { useId, useRef, type ReactNode } from 'react';
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
  /**
   * Called to put the slider back to its starting value: on a double click anywhere on the slider
   * (label, value, track or thumb), and on Delete or Backspace while the thumb has focus. The
   * slider does not change its value itself: the owner answers with `onChange`-like state. A
   * double click on the track still moves the thumb to the first click, as a single click does.
   * Without it, neither gesture does anything. Not called while `disabled`.
   */
  onReset?: () => void;
  /**
   * Text shown under the track, for example a warning about the current value. It is linked to the
   * thumb (`aria-describedby`), so assistive technology reads it as the slider's description, and
   * it is not a live region. Nothing is rendered for undefined, null, false or an empty string.
   */
  description?: ReactNode;
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
  onReset,
  description,
}: SliderProps) {
  const id = useId();
  const descriptionId = useId();
  const hasDescription =
    description !== undefined &&
    description !== null &&
    description !== false &&
    description !== '';
  const text = format(value);
  // For a key press Radix reports the release before the move, with the same value. Taking the
  // move after it as "still dragging" would leave a rough result on screen, so it is ignored.
  const released = useRef<number | null>(null);
  return (
    <div
      className={styles.slider}
      onDoubleClick={
        onReset && !disabled
          ? () => {
              onReset();
            }
          : undefined
      }
    >
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
        onKeyDown={(event) => {
          if (!onReset || disabled || (event.key !== 'Delete' && event.key !== 'Backspace')) return;
          event.preventDefault();
          onReset();
        }}
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
        <RadixSlider.Thumb
          className={styles.thumb}
          aria-labelledby={id}
          aria-describedby={hasDescription ? descriptionId : undefined}
          aria-valuetext={text}
        />
      </RadixSlider.Root>
      {hasDescription && (
        <p id={descriptionId} className={styles.description}>
          {description}
        </p>
      )}
    </div>
  );
}
