import { useTranslation } from 'react-i18next';
import { CurveEditor, Slider, type CurveChannel, type CurvePoint } from '@gtp/ui';
import type { Recipe } from '../../../shared/ipc';
import styles from '../develop.module.css';
import { curvesOf, toneOf, withCurve, withTone, type ToneKey } from '../recipe-parts';

/** The tone sliders that run from -100 to 100, in the order the tone stage applies them. */
const SLIDERS: readonly ToneKey[] = [
  'contrast',
  'highlights',
  'shadows',
  'whites',
  'blacks',
  'dehaze',
];
const EXPOSURE_MAX = 5;

interface TonePanelProps {
  recipe: Recipe;
  /** The library cannot be changed: everything is shown but nothing can be edited. */
  readOnly: boolean;
  /** Something is being done elsewhere in the view. */
  working: boolean;
  /** The recipe changed; `dragging` while a slider's thumb or a curve point is being moved. */
  onChange: (recipe: Recipe, options: { dragging: boolean }) => void;
}

/**
 * The Tone panel: exposure in stops, contrast, highlights, shadows, whites, blacks and dehaze, and
 * the tone curves (master and one per colour channel).
 */
export function TonePanel({ recipe, readOnly, working, onChange }: TonePanelProps) {
  const { t, i18n } = useTranslation();
  const tone = toneOf(recipe);
  const locked = readOnly || working;
  const whole = (value: number) =>
    new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 0 }).format(value);
  const hundredths = (value: number) =>
    new Intl.NumberFormat(i18n.language, {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(value);
  const stops = (value: number) => t('develop.tone.stops', { value: hundredths(value) });
  const untouched = Object.values(tone).every((v) => v === 0);

  const reset = () =>
    onChange(
      withTone(recipe, {
        exposure_ev: 0,
        contrast: 0,
        highlights: 0,
        shadows: 0,
        whites: 0,
        blacks: 0,
        dehaze: 0,
      }),
      { dragging: false },
    );

  return (
    <div className={styles.panel}>
      <section aria-labelledby="tone-heading" className={styles.panelSection}>
        <h3 id="tone-heading" className={styles.panelTitle}>
          {t('develop.tone.heading')}
        </h3>
        <div className={styles.sliders}>
          <Slider
            label={t('develop.tone.exposure_ev')}
            value={tone.exposure_ev}
            min={-EXPOSURE_MAX}
            max={EXPOSURE_MAX}
            step={0.01}
            format={stops}
            disabled={locked}
            onChange={(next, dragging) =>
              onChange(withTone(recipe, { exposure_ev: next }), { dragging })
            }
          />
          {SLIDERS.map((key) => (
            <Slider
              key={key}
              label={t(`develop.tone.${key}`)}
              value={tone[key]}
              min={-100}
              max={100}
              step={1}
              format={whole}
              disabled={locked}
              onChange={(next, dragging) =>
                onChange(withTone(recipe, { [key]: next }), { dragging })
              }
            />
          ))}
        </div>
        <div className={styles.panelButtons}>
          <button type="button" disabled={locked || untouched} onClick={reset}>
            {t('develop.tone.reset')}
          </button>
        </div>
      </section>

      <section aria-labelledby="curve-heading" className={styles.panelSection}>
        <h3 id="curve-heading" className={styles.panelTitle}>
          {t('develop.curve.heading')}
        </h3>
        <CurveEditor
          curves={curvesOf(recipe)}
          disabled={locked}
          onChange={(channel: CurveChannel, points: CurvePoint[], options) =>
            onChange(withCurve(recipe, channel, points), options)
          }
          text={{
            channels: t('develop.curve.channels'),
            master: t('develop.curve.master'),
            red: t('develop.curve.red'),
            green: t('develop.curve.green'),
            blue: t('develop.curve.blue'),
            plot: (channel) => t('develop.curve.plot', { channel }),
            unchanged: t('develop.curve.unchanged'),
            add: t('develop.curve.add'),
            remove: t('develop.curve.remove'),
            reset: t('develop.curve.reset'),
            point: (n, total) => t('develop.curve.point', { n, total }),
            value: (input, output) => t('develop.curve.value', { input, output }),
            number: hundredths,
          }}
        />
      </section>
    </div>
  );
}
