import { useTranslation } from 'react-i18next';
import { Slider } from '@gtp/ui';
import type { Recipe } from '../../../shared/ipc';
import styles from '../develop.module.css';
import {
  colourOf,
  monoOf,
  withColour,
  withMono,
  type Channel,
  type ColourKey,
} from '../recipe-parts';

/** The colour sliders that run from -100 to 100, in the order they are shown. */
const BALANCE: readonly ColourKey[] = [
  'red_cyan',
  'green_magenta',
  'blue_yellow',
  'temperature',
  'tint',
];
const SATURATION_MAX = 2;
const MIX_MAX = 2;
const MIX_STEP = 0.01;
const CHANNELS: ReadonlyArray<{ channel: Channel; name: 'red' | 'green' | 'blue' }> = [
  { channel: 0, name: 'red' },
  { channel: 1, name: 'green' },
  { channel: 2, name: 'blue' },
];

interface ColourPanelProps {
  recipe: Recipe;
  /** The library cannot be changed: everything is shown but nothing can be edited. */
  readOnly: boolean;
  /** Something is being done elsewhere in the view. */
  working: boolean;
  /** The neutral picker is on. */
  picking: boolean;
  /** The picture on screen is the one the pick is made on. */
  pickReady: boolean;
  onPickingChange: (on: boolean) => void;
  /** The recipe changed; `dragging` while a slider's thumb is being moved. */
  onChange: (recipe: Recipe, options: { dragging: boolean }) => void;
}

/**
 * The Colour panel: the three colour axes, temperature and tint, saturation and vibrance, a picker
 * that makes a clicked grey neutral, and a black-and-white mode with a channel mix. In
 * black-and-white mode saturation and vibrance do nothing, so they are not shown.
 */
export function ColourPanel({
  recipe,
  readOnly,
  working,
  picking,
  pickReady,
  onPickingChange,
  onChange,
}: ColourPanelProps) {
  const { t, i18n } = useTranslation();
  const colour = colourOf(recipe);
  const mono = monoOf(recipe);
  const locked = readOnly || working;
  const whole = (value: number) =>
    new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 0 }).format(value);
  const hundredths = (value: number) =>
    new Intl.NumberFormat(i18n.language, {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(value);

  return (
    <div className={styles.panel}>
      <section aria-labelledby="colour-heading" className={styles.panelSection}>
        <h3 id="colour-heading" className={styles.panelTitle}>
          {t('develop.colour.heading')}
        </h3>
        <div className={styles.panelButtons}>
          <button
            type="button"
            aria-pressed={picking}
            disabled={locked}
            onClick={() => onPickingChange(!picking)}
          >
            {t('develop.colour.pickNeutral')}
          </button>
        </div>
        {picking && (
          <p className={styles.panelText} role="status">
            {pickReady ? t('develop.colour.picking') : t('develop.colour.preparingFrame')}
          </p>
        )}
        <div className={styles.sliders}>
          {BALANCE.map((key) => (
            <Slider
              key={key}
              label={t(`develop.colour.${key}`)}
              value={colour[key]}
              min={-100}
              max={100}
              step={1}
              format={whole}
              disabled={locked}
              onChange={(next, dragging) =>
                onChange(withColour(recipe, { [key]: next }), { dragging })
              }
            />
          ))}
          {!mono.enabled && (
            <>
              <Slider
                label={t('develop.colour.saturation')}
                value={colour.saturation}
                min={0}
                max={SATURATION_MAX}
                step={MIX_STEP}
                format={hundredths}
                disabled={locked}
                onChange={(next, dragging) =>
                  onChange(withColour(recipe, { saturation: next }), { dragging })
                }
              />
              <Slider
                label={t('develop.colour.vibrance')}
                value={colour.vibrance}
                min={-100}
                max={100}
                step={1}
                format={whole}
                disabled={locked}
                onChange={(next, dragging) =>
                  onChange(withColour(recipe, { vibrance: next }), { dragging })
                }
              />
            </>
          )}
        </div>
      </section>

      <section aria-labelledby="mono-heading" className={styles.panelSection}>
        <h3 id="mono-heading" className={styles.panelTitle}>
          {t('develop.mono.heading')}
        </h3>
        <label className={styles.check}>
          <input
            type="checkbox"
            checked={mono.enabled}
            disabled={locked}
            onChange={(event) =>
              onChange(withMono(recipe, { ...mono, enabled: event.target.checked }), {
                dragging: false,
              })
            }
          />
          {t('develop.mono.enabled')}
        </label>
        {mono.enabled && (
          <div role="group" aria-label={t('develop.mono.mix')} className={styles.sliders}>
            {CHANNELS.map(({ channel, name }) => (
              <Slider
                key={name}
                label={t('develop.mono.weight', { channel: t(`develop.levels.${name}`) })}
                value={mono.mix[channel]}
                min={-MIX_MAX}
                max={MIX_MAX}
                step={MIX_STEP}
                format={hundredths}
                disabled={locked}
                onChange={(next, dragging) => {
                  const mix: [number, number, number] = [...mono.mix];
                  mix[channel] = next;
                  onChange(withMono(recipe, { ...mono, mix }), { dragging });
                }}
              />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
