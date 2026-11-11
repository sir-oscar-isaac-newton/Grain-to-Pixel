import { useTranslation } from 'react-i18next';
import { Slider } from '@gtp/ui';
import type { Recipe } from '../../../shared/ipc';
import styles from '../develop.module.css';
import {
  NOISE_MAX,
  SHARPEN_AMOUNT_MAX,
  SHARPEN_RADIUS_MAX,
  SHARPEN_RADIUS_MIN,
  noiseOf,
  sharpenOf,
  withNoise,
  withSharpen,
} from '../recipe-parts';
import { resetDetail } from '../recipe-reset';
import { ResetPanelButton } from './ResetPanelButton';

interface DetailPanelProps {
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
  /**
   * The recipe changed; `dragging` while a slider's thumb is being moved. `preview: false` when the
   * picture on screen would not change (output sharpening is applied at export).
   */
  onChange: (
    recipe: Recipe,
    options: { dragging: boolean; merge?: boolean; preview?: boolean },
  ) => void;
}

/**
 * Detail: noise reduction (luminance and colour), then output sharpening. The noise reduction is
 * drawn in the preview, which can only approximate it: fine grain shows only in the exported file.
 * Output sharpening is applied when the frame is exported, to the image at its final size, and not
 * to the preview, so nothing on screen changes when its sliders move; the panel says so.
 */
export function DetailPanel({
  recipe,
  readOnly,
  working,
  onChange,
  auto = null,
}: DetailPanelProps) {
  const { t, i18n } = useTranslation();
  const sharpen = sharpenOf(recipe);
  const noise = noiseOf(recipe);
  const locked = readOnly || working;
  const autoSharpen = auto ? sharpenOf(auto) : undefined;
  const autoNoise = auto ? noiseOf(auto) : undefined;
  const resetTo = (next: Recipe) => onChange(next, { dragging: false });
  // Sharpening is applied at export: the picture on screen would not change.
  const resetSharpenSlider = (next: Recipe) =>
    onChange(next, { dragging: false, merge: true, preview: false });
  const resetNoiseSlider = (next: Recipe) => onChange(next, { dragging: false, merge: true });
  const number = (digits: number) => (value: number) =>
    new Intl.NumberFormat(i18n.language, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    }).format(value);
  const percent = (value: number) => t('develop.sharpen.percent', { value: number(0)(value) });
  const pixels = (value: number) => t('develop.sharpen.pixels', { value: number(1)(value) });

  return (
    <div className={styles.panel}>
      <div className={styles.panelHead}>
        <h3 id="detail-heading" className={styles.panelTitle}>
          {t('develop.detail.heading')}
        </h3>
        <ResetPanelButton
          panel={t('develop.detail.resetName')}
          disabled={locked || !auto}
          onClick={() => auto && resetTo(resetDetail(recipe, auto))}
        />
      </div>
      <section aria-labelledby="noise-heading" className={styles.panelSection}>
        <h4 id="noise-heading" className={styles.panelTitle}>
          {t('develop.noise.heading')}
        </h4>
        <p id="noise-note" className={styles.panelText}>
          {t('develop.noise.note')}
        </p>
        <div
          role="group"
          aria-labelledby="noise-heading"
          aria-describedby="noise-note"
          className={styles.sliders}
        >
          <Slider
            label={t('develop.noise.luminance')}
            value={noise.luminance}
            min={0}
            max={NOISE_MAX}
            step={1}
            format={number(0)}
            disabled={locked}
            onReset={
              autoNoise &&
              (() => resetNoiseSlider(withNoise(recipe, { luminance: autoNoise.luminance })))
            }
            onChange={(next, dragging) =>
              onChange(withNoise(recipe, { luminance: next }), { dragging })
            }
          />
          <Slider
            label={t('develop.noise.color')}
            value={noise.color}
            min={0}
            max={NOISE_MAX}
            step={1}
            format={number(0)}
            disabled={locked}
            onReset={
              autoNoise && (() => resetNoiseSlider(withNoise(recipe, { color: autoNoise.color })))
            }
            onChange={(next, dragging) =>
              onChange(withNoise(recipe, { color: next }), { dragging })
            }
          />
        </div>
      </section>
      <section aria-labelledby="sharpen-heading" className={styles.panelSection}>
        <h4 id="sharpen-heading" className={styles.panelTitle}>
          {t('develop.sharpen.heading')}
        </h4>
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
              (() => resetSharpenSlider(withSharpen(recipe, { amount: autoSharpen.amount })))
            }
            onChange={(next, dragging) =>
              onChange(withSharpen(recipe, { amount: next }), { dragging, preview: false })
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
              (() => resetSharpenSlider(withSharpen(recipe, { radius_px: autoSharpen.radius_px })))
            }
            onChange={(next, dragging) =>
              onChange(withSharpen(recipe, { radius_px: Math.round(next * 10) / 10 }), {
                dragging,
                preview: false,
              })
            }
          />
        </div>
      </section>
    </div>
  );
}
