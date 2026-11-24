import { useTranslation } from 'react-i18next';
import { Slider } from '@gtp/ui';
import type { Recipe } from '../../../shared/ipc';
import styles from '../develop.module.css';
import { hueGradientCss, tintCss } from '../hue-colour';
import { splitToneOf, withSplitTone, type SplitToneKey } from '../recipe-parts';

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

interface SplitToneSlidersProps {
  recipe: Recipe;
  /** Nothing can be edited. */
  locked: boolean;
  /** The automatic recipe each slider's reset goes back to; the resets are off without it. */
  auto?: Recipe | null;
  onChange: (recipe: Recipe, options: { dragging: boolean; merge?: boolean }) => void;
}

/** The five split toning sliders, shared by the Colour panel and the Black and white panel. */
export function SplitToneSliders({ recipe, locked, auto = null, onChange }: SplitToneSlidersProps) {
  const { t, i18n } = useTranslation();
  const split = splitToneOf(recipe);
  const autoSplit = auto ? splitToneOf(auto) : undefined;
  const whole = (value: number) =>
    new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 0 }).format(value);
  const degrees = (value: number) => `${whole(value)}°`;
  const gradient = hueGradientCss();
  return (
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
          swatch={hue ? tintCss(split[key]) : undefined}
          trackGradient={hue ? gradient : undefined}
          disabled={locked}
          onReset={
            autoSplit &&
            (() =>
              onChange(withSplitTone(recipe, { [key]: autoSplit[key] }), {
                dragging: false,
                merge: true,
              }))
          }
          onChange={(next, dragging) =>
            onChange(withSplitTone(recipe, { [key]: next }), { dragging })
          }
        />
      ))}
    </div>
  );
}
