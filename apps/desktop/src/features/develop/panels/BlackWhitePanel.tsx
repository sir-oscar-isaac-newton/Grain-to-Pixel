import { useTranslation } from 'react-i18next';
import type { Recipe } from '../../../shared/ipc';
import styles from '../develop.module.css';
import {
  PAPER_GRADES,
  TONING_PRESETS,
  splitToneOf,
  toneOf,
  withSplitTone,
  withTone,
  type SplitToneSetting,
  type ToningName,
} from '../recipe-parts';
import { resetBlackWhite } from '../recipe-reset';
import { ResetPanelButton } from './ResetPanelButton';
import { SplitToneSliders } from './SplitToneSliders';

const GRADES = [0, 1, 2, 3, 4, 5] as const;
const TONINGS = Object.keys(TONING_PRESETS) as ToningName[];

const sameTone = (a: SplitToneSetting, b: SplitToneSetting) =>
  (Object.keys(b) as Array<keyof SplitToneSetting>).every((key) => a[key] === b[key]);

interface BlackWhitePanelProps {
  recipe: Recipe;
  /** The library cannot be changed: everything is shown but nothing can be edited. */
  readOnly: boolean;
  /** Something is being done elsewhere in the view. */
  working: boolean;
  /** The automatic recipe: what the toning sliders and "Reset" go back to. */
  auto?: Recipe | null;
  /** The recipe changed; `dragging` while a slider's thumb is being moved. */
  onChange: (recipe: Recipe, options: { dragging: boolean; merge?: boolean }) => void;
}

/**
 * The Black and white panel, shown instead of the Colour panel for a black-and-white frame: the
 * paper grade (a set of contrast steps, which are the Tone panel's contrast) and the toning (presets
 * that set the five split toning values, which stay editable below). The colour controls do
 * nothing for such a frame, so they are not shown.
 */
export function BlackWhitePanel({
  recipe,
  readOnly,
  working,
  auto = null,
  onChange,
}: BlackWhitePanelProps) {
  const { t } = useTranslation();
  const locked = readOnly || working;
  const contrast = toneOf(recipe).contrast;
  const split = splitToneOf(recipe);

  return (
    <div className={styles.panel}>
      <section aria-labelledby="bw-heading" className={styles.panelSection}>
        <div className={styles.panelHead}>
          <h3 id="bw-heading" className={styles.panelTitle}>
            {t('develop.blackWhite.heading')}
          </h3>
          <ResetPanelButton
            panel={t('develop.blackWhite.heading')}
            disabled={locked || !auto}
            onClick={() => auto && onChange(resetBlackWhite(recipe, auto), { dragging: false })}
          />
        </div>
      </section>
      <section aria-labelledby="bw-grade-heading" className={styles.panelSection}>
        <h4 id="bw-grade-heading" className={styles.panelTitle}>
          {t('develop.blackWhite.gradeHeading')}
        </h4>
        <div
          role="group"
          aria-labelledby="bw-grade-heading"
          aria-describedby="bw-grade-hint"
          className={styles.panelButtons}
        >
          {GRADES.map((grade) => (
            <button
              key={grade}
              type="button"
              aria-pressed={contrast === PAPER_GRADES[grade]}
              disabled={locked}
              onClick={() =>
                onChange(withTone(recipe, { contrast: PAPER_GRADES[grade] }), { dragging: false })
              }
            >
              {t('develop.blackWhite.grade', { n: grade })}
            </button>
          ))}
        </div>
        <p id="bw-grade-hint" className={styles.panelText}>
          {t('develop.blackWhite.gradeHint')}
        </p>
      </section>
      <section aria-labelledby="bw-toning-heading" className={styles.panelSection}>
        <h4 id="bw-toning-heading" className={styles.panelTitle}>
          {t('develop.blackWhite.toningHeading')}
        </h4>
        <div role="group" aria-labelledby="bw-toning-heading" className={styles.panelButtons}>
          {TONINGS.map((name) => (
            <button
              key={name}
              type="button"
              aria-pressed={sameTone(split, TONING_PRESETS[name])}
              disabled={locked}
              onClick={() =>
                onChange(withSplitTone(recipe, TONING_PRESETS[name]), { dragging: false })
              }
            >
              {t(`develop.blackWhite.toning.${name}`)}
            </button>
          ))}
        </div>
        <SplitToneSliders recipe={recipe} auto={auto} locked={locked} onChange={onChange} />
      </section>
    </div>
  );
}
