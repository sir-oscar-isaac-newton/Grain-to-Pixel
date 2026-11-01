import { useTranslation } from 'react-i18next';
import { Slider } from '@gtp/ui';
import type { Recipe } from '../../../shared/ipc';
import styles from '../develop.module.css';
import {
  SHARPEN_AMOUNT_MAX,
  SHARPEN_DEFAULT,
  SHARPEN_RADIUS_MAX,
  SHARPEN_RADIUS_MIN,
  sharpenOf,
  withSharpen,
} from '../recipe-parts';

interface SharpenPanelProps {
  recipe: Recipe;
  /** The library cannot be changed: everything is shown but nothing can be edited. */
  readOnly: boolean;
  /** Something is being done elsewhere in the view. */
  working: boolean;
  /** The recipe changed; `dragging` while a slider's thumb is being moved. */
  onChange: (recipe: Recipe, options: { dragging: boolean }) => void;
}

/**
 * Output sharpening: how much, and with what radius. It is applied when the frame is exported, to
 * the image at its final size, and not to the preview, so nothing on screen changes when a slider
 * moves; the panel says so.
 */
export function SharpenPanel({ recipe, readOnly, working, onChange }: SharpenPanelProps) {
  const { t, i18n } = useTranslation();
  const sharpen = sharpenOf(recipe);
  const locked = readOnly || working;
  const number = (digits: number) => (value: number) =>
    new Intl.NumberFormat(i18n.language, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    }).format(value);
  const percent = (value: number) => t('develop.sharpen.percent', { value: number(0)(value) });
  const pixels = (value: number) => t('develop.sharpen.pixels', { value: number(1)(value) });
  const atDefaults =
    sharpen.amount === SHARPEN_DEFAULT.amount && sharpen.radius_px === SHARPEN_DEFAULT.radius_px;

  return (
    <div className={styles.panel}>
      <section aria-labelledby="sharpen-heading" className={styles.panelSection}>
        <h3 id="sharpen-heading" className={styles.panelTitle}>
          {t('develop.sharpen.heading')}
        </h3>
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
            onChange={(next, dragging) =>
              onChange(withSharpen(recipe, { radius_px: Math.round(next * 10) / 10 }), {
                dragging,
              })
            }
          />
        </div>
        <div className={styles.panelButtons}>
          <button
            type="button"
            disabled={locked || atDefaults}
            onClick={() => onChange(withSharpen(recipe, SHARPEN_DEFAULT), { dragging: false })}
          >
            {t('develop.sharpen.reset')}
          </button>
        </div>
      </section>
    </div>
  );
}
