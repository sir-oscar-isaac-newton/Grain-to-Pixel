import { useTranslation } from 'react-i18next';
import { Slider } from '@gtp/ui';
import type { Recipe } from '../../../shared/ipc';
import { FULL_CROP, MIN_CROP, edgesOf, moveEdge, sameCrop, type Edge } from '../crop';
import styles from '../develop.module.css';
import { geometryOf, withGeometry } from '../recipe-parts';
import type { DetectedFrame } from '../use-develop';

/** Below this the frame the analysis found is not trusted (the backend's own limit). */
export const LOW_CONFIDENCE = 0.5;
/** Straighten runs from minus this to plus this, in degrees (the recipe's limit). */
const ANGLE_MAX = 45;
const EDGES: readonly Edge[] = ['left', 'top', 'right', 'bottom'];

interface GeometryPanelProps {
  recipe: Recipe;
  /** The library cannot be changed: everything is shown but nothing can be edited. */
  readOnly: boolean;
  /** Something is being done elsewhere in the view. */
  working: boolean;
  /** The crop is being edited on the whole scan. */
  cropping: boolean;
  /** The picture on screen is the whole scan, so the crop can be edited on it. */
  cropReady: boolean;
  onCroppingChange: (on: boolean) => void;
  /** The frame the analysis found; null until it is done. */
  detected: DetectedFrame | null;
  /** The straighten slider is in use, so a grid helps to judge a horizon. */
  onGuideChange: (on: boolean) => void;
  /** The recipe changed; `dragging` while a slider's thumb is being moved. */
  onChange: (recipe: Recipe, options: { dragging: boolean }) => void;
}

/**
 * The Geometry panel: the crop (edited on the whole scan, with sliders for its four sides), a
 * quarter turn either way, flips and a straighten angle. A hint says when the frame could not be
 * found reliably and the whole scan is used.
 */
export function GeometryPanel({
  recipe,
  readOnly,
  working,
  cropping,
  cropReady,
  onCroppingChange,
  detected,
  onGuideChange,
  onChange,
}: GeometryPanelProps) {
  const { t, i18n } = useTranslation();
  const geometry = geometryOf(recipe);
  const locked = readOnly || working;
  const edges = edgesOf(geometry.crop);
  const percent = (value: number) =>
    new Intl.NumberFormat(i18n.language, {
      style: 'percent',
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    }).format(value);
  const degrees = (value: number) =>
    t('develop.geometry.degrees', {
      value: new Intl.NumberFormat(i18n.language, {
        minimumFractionDigits: 1,
        maximumFractionDigits: 1,
      }).format(value),
    });
  const change = (values: Parameters<typeof withGeometry>[1], dragging = false) =>
    onChange(withGeometry(recipe, values), { dragging });

  const confident = detected !== null && detected.confidence >= LOW_CONFIDENCE;
  const turned = geometry.rot90 !== 0 || geometry.flip_h || geometry.flip_v;
  const levelled = geometry.angle_deg === 0;

  /** The range one side may be set to: it stays {@link MIN_CROP} from the opposite side. */
  const range = (edge: Edge): [number, number] => {
    switch (edge) {
      case 'left':
        return [0, edges.right - MIN_CROP];
      case 'right':
        return [edges.left + MIN_CROP, 1];
      case 'top':
        return [0, edges.bottom - MIN_CROP];
      case 'bottom':
        return [edges.top + MIN_CROP, 1];
    }
  };

  return (
    <div className={styles.panel}>
      <section aria-labelledby="crop-heading" className={styles.panelSection}>
        <h3 id="crop-heading" className={styles.panelTitle}>
          {t('develop.crop.heading')}
        </h3>
        {detected !== null && !confident && (
          <p className={styles.panelText} role="note">
            {t('develop.crop.lowConfidence')}
          </p>
        )}
        <div className={styles.panelButtons}>
          <button
            type="button"
            aria-pressed={cropping}
            disabled={locked}
            onClick={() => onCroppingChange(!cropping)}
          >
            {t('develop.crop.edit')}
          </button>
          <button
            type="button"
            disabled={locked || !confident || sameCrop(geometry.crop, detected.rect)}
            onClick={() => change({ crop: detected ? detected.rect : FULL_CROP })}
          >
            {t('develop.crop.useDetected')}
          </button>
          <button
            type="button"
            disabled={locked || sameCrop(geometry.crop, FULL_CROP)}
            onClick={() => change({ crop: FULL_CROP })}
          >
            {t('develop.crop.useWhole')}
          </button>
        </div>
        {cropping && (
          <p className={styles.panelText} role="status">
            {cropReady ? t('develop.crop.editing') : t('develop.crop.preparing')}
          </p>
        )}
        <div className={styles.sliders}>
          {EDGES.map((edge) => {
            const [min, max] = range(edge);
            return (
              <Slider
                key={edge}
                label={t(`develop.crop.${edge}`)}
                value={edges[edge]}
                min={min}
                max={max}
                step={0.001}
                format={percent}
                disabled={locked}
                onChange={(next, dragging) =>
                  change({ crop: moveEdge(geometry.crop, edge, next) }, dragging)
                }
              />
            );
          })}
        </div>
      </section>

      <section aria-labelledby="turn-heading" className={styles.panelSection}>
        <h3 id="turn-heading" className={styles.panelTitle}>
          {t('develop.geometry.heading')}
        </h3>
        <div className={styles.panelButtons}>
          <button
            type="button"
            disabled={locked}
            onClick={() => change({ rot90: (geometry.rot90 + 3) % 4 })}
          >
            {t('develop.geometry.rotateLeft')}
          </button>
          <button
            type="button"
            disabled={locked}
            onClick={() => change({ rot90: (geometry.rot90 + 1) % 4 })}
          >
            {t('develop.geometry.rotateRight')}
          </button>
          <button
            type="button"
            aria-pressed={geometry.flip_h}
            disabled={locked}
            onClick={() => change({ flip_h: !geometry.flip_h })}
          >
            {t('develop.geometry.flipH')}
          </button>
          <button
            type="button"
            aria-pressed={geometry.flip_v}
            disabled={locked}
            onClick={() => change({ flip_v: !geometry.flip_v })}
          >
            {t('develop.geometry.flipV')}
          </button>
        </div>
        <div onFocus={() => onGuideChange(true)} onBlur={() => onGuideChange(false)}>
          <Slider
            label={t('develop.geometry.straighten')}
            value={geometry.angle_deg}
            min={-ANGLE_MAX}
            max={ANGLE_MAX}
            step={0.1}
            format={degrees}
            disabled={locked}
            onChange={(next, dragging) => change({ angle_deg: next }, dragging)}
          />
        </div>
        <div className={styles.panelButtons}>
          <button
            type="button"
            disabled={locked || (!turned && levelled)}
            onClick={() => change({ rot90: 0, flip_h: false, flip_v: false, angle_deg: 0 })}
          >
            {t('develop.geometry.reset')}
          </button>
        </div>
      </section>
    </div>
  );
}
