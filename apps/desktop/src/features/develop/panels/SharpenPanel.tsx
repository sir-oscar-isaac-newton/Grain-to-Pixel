import { useTranslation } from 'react-i18next';
import { Slider } from '@gtp/ui';
import type { Recipe } from '../../../shared/ipc';
import styles from '../develop.module.css';
import {
  SHARPEN_AMOUNT_MAX,
  SHARPEN_RADIUS_MAX,
  SHARPEN_RADIUS_MIN,
  sharpenOf,
  withSharpen,
} from '../recipe-parts';
import { resetSharpen } from '../recipe-reset';
import { ResetPanelButton } from './ResetPanelButton';

interface SharpenPanelProps {
  recipe: Recipe;
  /** The library cannot be changed: everything is shown but nothing can be edited. */
  readOnly: boolean;
  /** Something is being done elsewhere in the view. */
  working: boolean;
  /**
   * The automatic recipe: what the sliders and "Reset" go back to. Null or missing until it is
   * known; the resets are then off.
   */
  auto?: Recipe | null;
  /** The recipe changed; `dragging` while a slider's thumb is being moved. */
  onChange: (recipe: Recipe, options: { dragging: boolean; merge?: boolean }) => void;
}

/**
 * Output sharpening: how much, and with what radius. It is applied when the frame is exported, to
 * the image at its final size, and not to the preview, so nothing on screen changes when a slider
 * moves; the panel says so.
 */
export function SharpenPanel({
  recipe,
  readOnly,
  working,
  onChange,
  auto = null,
}: SharpenPanelProps) {
  const { t, i18n } = useTranslation();
  const sharpen = sharpenOf(recipe);
  const locked = readOnly || working;
  const autoSharpen = auto ? sharpenOf(auto) : undefined;
  const resetTo = (next: Recipe) => onChange(next, { dragging: false });
  const resetSlider = (next: Recipe) => onChange(next, { dragging: false, merge: true });
  const number = (digits: number) => (value: number) =>
    new Intl.NumberFormat(i18n.language, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    }).format(value);
  const percent = (value: number) => t('develop.sharpen.percent', { value: number(0)(value) });
  const pixels = (value: number) => t('develop.sharpen.pixels', { value: number(1)(value) });

  return (
    <div className={styles.panel}>
      <section aria-labelledby="sharpen-heading" className={styles.panelSection}>
        <div className={styles.panelHead}>
          <h3 id="sharpen-heading" className={styles.panelTitle}>
            {t('develop.sharpen.heading')}
          </h3>
          <ResetPanelButton
            panel={t('develop.sharpen.heading')}
            disabled={locked || !auto}
            onClick={() => auto && resetTo(resetSharpen(recipe, auto))}
          />
        </div>
        <p id="sharpen-note" className={styles.panelText}>
          {t('develop.sharpen.note')}
        </p>
        <div
          role="group"
          aria-labelledby="sharpen-heading"
          aria-describedby="sharpen-note"
          className={styles.sliders}
        >
          <Slider
            label={t('develop.sharpen.amount')}
            value={sharpen.amount}
            min={0}
            max={SHARPEN_AMOUNT_MAX}
            step={1}
            format={percent}
            disabled={locked}
            onReset={
              autoSharpen &&
              (() => resetSlider(withSharpen(recipe, { amount: autoSharpen.amount })))
            }
            onChange={(next, dragging) =>
              onChange(withSharpen(recipe, { amount: next }), { dragging })
            }
          />
          <Slider
            label={t('develop.sharpen.radius')}
            value={sharpen.radius_px}
            min={SHARPEN_RADIUS_MIN}
            max={SHARPEN_RADIUS_MAX}
            step={0.1}
            format={pixels}
            disabled={locked}
            onReset={
              autoSharpen &&
              (() => resetSlider(withSharpen(recipe, { radius_px: autoSharpen.radius_px })))
            }
            onChange={(next, dragging) =>
              onChange(withSharpen(recipe, { radius_px: Math.round(next * 10) / 10 }), {
                dragging,
              })
            }
          />
        </div>
      </section>
    </div>
  );
}
