import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Slider } from '@gtp/ui';
import type { Recipe } from '../../../shared/ipc';
import styles from '../develop.module.css';
import {
  colourOf,
  mixerOf,
  monoOf,
  splitToneOf,
  withColour,
  withMixerValue,
  withMono,
  withSplitTone,
  MIXER_BANDS,
  MIXER_MODES,
  type MixerMode,
  type SplitToneKey,
  type Channel,
  type ColourKey,
} from '../recipe-parts';
import { resetColour } from '../recipe-reset';
import { ResetPanelButton } from './ResetPanelButton';

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
/** The split toning sliders in the order they are shown: label key, range and whether it is a hue. */
const SPLIT_TONE_SLIDERS: ReadonlyArray<{
  key: SplitToneKey;
  min: number;
  max: number;
  hue: boolean;
}> = [
  { key: 'highlight_hue', min: 0, max: 360, hue: true },
  { key: 'highlight_strength', min: 0, max: 100, hue: false },
  { key: 'shadow_hue', min: 0, max: 360, hue: true },
  { key: 'shadow_strength', min: 0, max: 100, hue: false },
  { key: 'balance', min: -100, max: 100, hue: false },
];
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
  /**
   * The automatic recipe: what the sliders and "Reset" go back to. Null or missing until it is
   * known; the resets are then off.
   */
  auto?: Recipe | null;
  /** The recipe changed; `dragging` while a slider's thumb is being moved. */
  onChange: (recipe: Recipe, options: { dragging: boolean; merge?: boolean }) => void;
}

/**
 * The Colour panel: the three colour axes, temperature and tint, saturation and vibrance, a picker
 * that makes a clicked grey neutral, a black-and-white mode with a channel mix, the colour mixer
 * (hue, saturation or lightness for each of eight bands of colours) and split toning. In
 * black-and-white mode saturation, vibrance and the colour mixer do nothing, so they are not shown;
 * split toning still works there and stays.
 */
export function ColourPanel({
  recipe,
  readOnly,
  working,
  picking,
  pickReady,
  onPickingChange,
  onChange,
  auto = null,
}: ColourPanelProps) {
  const { t, i18n } = useTranslation();
  const colour = colourOf(recipe);
  const mono = monoOf(recipe);
  const locked = readOnly || working;
  const autoColour = auto ? colourOf(auto) : undefined;
  const autoMix = auto ? monoOf(auto).mix : undefined;
  const split = splitToneOf(recipe);
  const autoSplit = auto ? splitToneOf(auto) : undefined;
  const [mixerMode, setMixerMode] = useState<MixerMode>('hue');
  const mixer = mixerOf(recipe);
  const autoMixer = auto ? mixerOf(auto) : undefined;
  const resetTo = (next: Recipe) => onChange(next, { dragging: false });
  const resetSlider = (next: Recipe) => onChange(next, { dragging: false, merge: true });
  const whole = (value: number) =>
    new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 0 }).format(value);
  const degrees = (value: number) => `${whole(value)}°`;
  const hundredths = (value: number) =>
    new Intl.NumberFormat(i18n.language, {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(value);

  return (
    <div className={styles.panel}>
      <section aria-labelledby="colour-heading" className={styles.panelSection}>
        <div className={styles.panelHead}>
          <h3 id="colour-heading" className={styles.panelTitle}>
            {t('develop.colour.heading')}
          </h3>
          <ResetPanelButton
            panel={t('develop.colour.heading')}
            disabled={locked || !auto}
            onClick={() => auto && resetTo(resetColour(recipe, auto))}
          />
        </div>
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
              onReset={
                autoColour && (() => resetSlider(withColour(recipe, { [key]: autoColour[key] })))
              }
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
                onReset={
                  autoColour &&
                  (() => resetSlider(withColour(recipe, { saturation: autoColour.saturation })))
                }
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
                onReset={
                  autoColour &&
                  (() => resetSlider(withColour(recipe, { vibrance: autoColour.vibrance })))
                }
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
                onReset={
                  autoMix &&
                  (() => {
                    const mix: [number, number, number] = [...mono.mix];
                    mix[channel] = autoMix[channel];
                    resetSlider(withMono(recipe, { ...mono, mix }));
                  })
                }
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

      {!mono.enabled && (
        <section aria-labelledby="mixer-heading" className={styles.panelSection}>
          <h3 id="mixer-heading" className={styles.panelTitle}>
            {t('develop.mixer.heading')}
          </h3>
          <div role="group" aria-label={t('develop.mixer.mode')} className={styles.panelButtons}>
            {MIXER_MODES.map((mode) => (
              <button
                key={mode}
                type="button"
                aria-pressed={mixerMode === mode}
                onClick={() => setMixerMode(mode)}
              >
                {t(`develop.mixer.${mode}`)}
              </button>
            ))}
          </div>
          <div className={styles.sliders}>
            {MIXER_BANDS.map((band, index) => (
              <Slider
                key={`${mixerMode}-${band}`}
                label={t(`develop.mixer.band.${band}`)}
                value={mixer[mixerMode][index] ?? 0}
                min={-100}
                max={100}
                step={1}
                format={whole}
                disabled={locked}
                onReset={
                  autoMixer &&
                  (() =>
                    resetSlider(
                      withMixerValue(recipe, mixerMode, index, autoMixer[mixerMode][index] ?? 0),
                    ))
                }
                onChange={(next, dragging) =>
                  onChange(withMixerValue(recipe, mixerMode, index, next), { dragging })
                }
              />
            ))}
          </div>
        </section>
      )}

      <section aria-labelledby="split-tone-heading" className={styles.panelSection}>
        <h3 id="split-tone-heading" className={styles.panelTitle}>
          {t('develop.splitTone.heading')}
        </h3>
        <div className={styles.sliders}>
          {SPLIT_TONE_SLIDERS.map(({ key, min, max, hue }) => (
            <Slider
              key={key}
              label={t(`develop.splitTone.${key}`)}
              value={split[key]}
              min={min}
              max={max}
              step={1}
              format={hue ? degrees : whole}
              disabled={locked}
              onReset={
                autoSplit && (() => resetSlider(withSplitTone(recipe, { [key]: autoSplit[key] })))
              }
              onChange={(next, dragging) =>
                onChange(withSplitTone(recipe, { [key]: next }), { dragging })
              }
            />
          ))}
        </div>
      </section>
    </div>
  );
}
