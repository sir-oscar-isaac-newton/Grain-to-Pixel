import { useTranslation } from 'react-i18next';
import { Slider } from '@gtp/ui';
import type { LevelsPoint, Recipe } from '../../../shared/ipc';
import styles from '../develop.module.css';
import {
  baseOf,
  levelsOf,
  withBase,
  withLevel,
  type Channel,
  type LevelKind,
  type Triple,
} from '../recipe-parts';

/** The levels sliders run from 0 to at least this, and to the value if it is higher. */
const LEVEL_SLIDER_MAX = 4;
const LEVEL_STEP = 0.005;
const KINDS: readonly LevelKind[] = ['black', 'white'];
const POINTS: readonly LevelsPoint[] = ['white', 'black'];
const CHANNELS: ReadonlyArray<{ channel: Channel; name: 'red' | 'green' | 'blue' }> = [
  { channel: 0, name: 'red' },
  { channel: 1, name: 'green' },
  { channel: 2, name: 'blue' },
];

interface BaseInvertPanelProps {
  recipe: Recipe;
  /** The roll's film base, if it has one. */
  rollBase: Triple | null;
  /** The library cannot be changed: everything is shown but nothing can be edited. */
  readOnly: boolean;
  /** What is being picked on the picture, if anything. */
  pick: 'base' | LevelsPoint | null;
  /** The picture on screen is the one the pick is made on. */
  pickReady: boolean;
  onPickChange: (pick: 'base' | LevelsPoint | null) => void;
  /** The recipe changed; `dragging` while a slider's thumb is being moved. */
  onChange: (recipe: Recipe, options: { dragging: boolean }) => void;
  onAutoLevels: () => void;
  onUseAsRollDefault: () => void;
  /** Something is being done (finding auto levels, setting the roll's base). */
  working: boolean;
}

/**
 * The Base and Invert panel: where the film base comes from, a picker to sample it on the rebate,
 * a button to make it the roll's default, and the black and white points of the inversion, which
 * the automatic levels fill in and the sliders override.
 */
export function BaseInvertPanel({
  recipe,
  rollBase,
  readOnly,
  pick,
  pickReady,
  onPickChange,
  onChange,
  onAutoLevels,
  onUseAsRollDefault,
  working,
}: BaseInvertPanelProps) {
  const { t, i18n } = useTranslation();
  const base = baseOf(recipe);
  const levels = levelsOf(recipe);
  const number = (value: number) =>
    new Intl.NumberFormat(i18n.language, {
      minimumFractionDigits: 3,
      maximumFractionDigits: 3,
    }).format(value);
  const shown = base.source === 'manual' ? base.rgb : (rollBase ?? base.rgb);
  const locked = readOnly || working;
  const picking = pick === 'base';

  return (
    <div className={styles.panel}>
      <section aria-labelledby="base-heading" className={styles.panelSection}>
        <h3 id="base-heading" className={styles.panelTitle}>
          {t('develop.base.heading')}
        </h3>
        <p className={styles.panelText}>
          {base.source === 'manual'
            ? t('develop.base.manual')
            : rollBase
              ? t('develop.base.fromRoll')
              : t('develop.base.fromRollNone')}
        </p>
        <p className={styles.panelValues}>
          {t('develop.base.values', {
            r: number(shown[0]),
            g: number(shown[1]),
            b: number(shown[2]),
          })}
        </p>
        <div className={styles.panelButtons}>
          <button
            type="button"
            aria-pressed={picking}
            disabled={locked}
            onClick={() => onPickChange(picking ? null : 'base')}
          >
            {t('develop.base.pick')}
          </button>
          <button
            type="button"
            disabled={locked || base.source === 'roll'}
            onClick={() =>
              onChange(withBase(recipe, { ...base, source: 'roll' }), { dragging: false })
            }
          >
            {t('develop.base.useRoll')}
          </button>
          <button
            type="button"
            disabled={locked || base.source !== 'manual'}
            onClick={onUseAsRollDefault}
          >
            {t('develop.base.makeRollDefault')}
          </button>
        </div>
        {picking && (
          <p className={styles.panelText} role="status">
            {pickReady ? t('develop.base.picking') : t('develop.base.preparingScan')}
          </p>
        )}
      </section>

      <section aria-labelledby="levels-heading" className={styles.panelSection}>
        <h3 id="levels-heading" className={styles.panelTitle}>
          {t('develop.levels.heading')}
        </h3>
        <div className={styles.panelButtons}>
          <button type="button" disabled={locked} onClick={onAutoLevels}>
            {t('develop.levels.auto')}
          </button>
          {POINTS.map((point) => (
            <button
              key={point}
              type="button"
              aria-pressed={pick === point}
              disabled={locked}
              onClick={() => onPickChange(pick === point ? null : point)}
            >
              {t(point === 'white' ? 'develop.levels.pickWhite' : 'develop.levels.pickBlack')}
            </button>
          ))}
        </div>
        {(pick === 'white' || pick === 'black') && (
          <p className={styles.panelText} role="status">
            {pickReady
              ? t(pick === 'white' ? 'develop.levels.pickingWhite' : 'develop.levels.pickingBlack')
              : t('develop.levels.preparingFrame')}
          </p>
        )}
        {KINDS.map((kind) => (
          <div
            key={kind}
            role="group"
            aria-label={t(`develop.levels.${kind}`)}
            className={styles.sliders}
          >
            {CHANNELS.map(({ channel, name }) => {
              const value = levels[kind][channel];
              return (
                <Slider
                  key={name}
                  label={t('develop.levels.slider', {
                    kind: t(`develop.levels.${kind}`),
                    channel: t(`develop.levels.${name}`),
                  })}
                  value={value}
                  min={0}
                  max={Math.max(LEVEL_SLIDER_MAX, Math.ceil(value))}
                  step={LEVEL_STEP}
                  format={number}
                  disabled={locked}
                  onChange={(next, dragging) =>
                    onChange(withLevel(recipe, kind, channel, next), { dragging })
                  }
                />
              );
            })}
          </div>
        ))}
      </section>
    </div>
  );
}
