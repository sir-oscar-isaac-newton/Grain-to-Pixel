import { useTranslation } from 'react-i18next';
import { Slider } from '@gtp/ui';
import type { Recipe } from '../../../shared/ipc';
import styles from '../develop.module.css';
import {
  DUST_DEFAULT,
  DUST_GROW_MAX,
  dustOf,
  sensitivityOf,
  thresholdOf,
  withDust,
} from '../recipe-parts';

interface DustPanelProps {
  recipe: Recipe;
  /** The library cannot be changed: everything is shown but nothing can be edited. */
  readOnly: boolean;
  /** Something is being done elsewhere in the view. */
  working: boolean;
  /** The `dust_removal` feature flag of the settings, or null when it could not be read. */
  flagOn: boolean | null;
  /** The recipe changed; `dragging` while a slider's thumb is being moved. */
  onChange: (recipe: Recipe, options: { dragging: boolean }) => void;
}

/**
 * Dust removal with the infrared plane: on or off, how sensitive the search for dust is, and how
 * far the spots are widened before they are filled in. The view shows this panel only for frames
 * that have an infrared plane. The sensitivity is the recipe's threshold turned around, so more
 * is more dust found.
 */
export function DustPanel({ recipe, readOnly, working, flagOn, onChange }: DustPanelProps) {
  const { t, i18n } = useTranslation();
  const dust = dustOf(recipe);
  const locked = readOnly || working;
  const sensitivity = sensitivityOf(dust.threshold);
  const atDefaults =
    dust.enabled === DUST_DEFAULT.enabled &&
    dust.threshold === DUST_DEFAULT.threshold &&
    dust.grow_px === DUST_DEFAULT.grow_px;
  const number = (value: number) => new Intl.NumberFormat(i18n.language).format(value);

  return (
    <div className={styles.panel}>
      <section aria-labelledby="dust-heading" className={styles.panelSection}>
        <h3 id="dust-heading" className={styles.panelTitle}>
          {t('develop.dust.heading')}
        </h3>
        <p id="dust-note" className={styles.panelText}>
          {t('develop.dust.note')}
        </p>
        {flagOn === false && (
          <p role="status" className={styles.panelText}>
            {t('develop.dust.flagOff')}
          </p>
        )}
        <label className={styles.check}>
          <input
            type="checkbox"
            checked={dust.enabled}
            disabled={locked}
            onChange={(event) =>
              onChange(withDust(recipe, { enabled: event.target.checked }), { dragging: false })
            }
          />
          {t('develop.dust.enabled')}
        </label>
        <div
          role="group"
          aria-labelledby="dust-heading"
          aria-describedby="dust-note"
          className={styles.sliders}
        >
          <Slider
            label={t('develop.dust.sensitivity')}
            value={sensitivity}
            min={0}
            max={100}
            step={1}
            format={(value) => t('develop.dust.percent', { value: number(value) })}
            disabled={locked || !dust.enabled}
            onChange={(next, dragging) =>
              onChange(withDust(recipe, { threshold: thresholdOf(next) }), { dragging })
            }
          />
          <Slider
            label={t('develop.dust.grow')}
            value={dust.grow_px}
            min={0}
            max={DUST_GROW_MAX}
            step={1}
            format={(value) => t('develop.dust.pixels', { value: number(value) })}
            disabled={locked || !dust.enabled}
            onChange={(next, dragging) =>
              onChange(withDust(recipe, { grow_px: Math.round(next) }), { dragging })
            }
          />
        </div>
        <div className={styles.panelButtons}>
          <button
            type="button"
            disabled={locked || atDefaults}
            onClick={() => onChange(withDust(recipe, DUST_DEFAULT), { dragging: false })}
          >
            {t('develop.dust.reset')}
          </button>
        </div>
      </section>
    </div>
  );
}
