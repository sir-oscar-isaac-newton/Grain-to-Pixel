import { useTranslation } from 'react-i18next';
import { Slider } from '@gtp/ui';
import type { Recipe } from '../../../shared/ipc';
import {
  FULL_CROP,
  MIN_CROP,
  edgesOf,
  moveEdge,
  sameCrop,
  type CropRect,
  type Edge,
} from '../crop';
import {
  PRESET_MODES,
  customRatio,
  finishedRatio,
  fitInside,
  fitLargest,
  fractionRatio,
  moveEdgeLocked,
  ratioInScan,
  type RatioMode,
  type RatioState,
  type ScanSize,
} from '../crop-ratio';
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
  /** The aspect ratio the crop is locked to, if any (an editing aid: it is not stored). */
  ratio: RatioState;
  onRatioChange: (ratio: RatioState) => void;
  /** The size of the scan, which the ratio of the crop depends on; null until it is known. */
  scan: ScanSize | null;
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
  ratio,
  onRatioChange,
  scan,
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

  /** The crop's width over height, as fractions of the scan, for a ratio setting and a turn. */
  const fractionsFor = (state: RatioState, rot90: number): number | null => {
    if (!scan) return null;
    const finished = finishedRatio(state, scan, rot90);
    return finished === null ? null : fractionRatio(ratioInScan(finished, rot90), scan);
  };
  const k = fractionsFor(ratio, geometry.rot90);
  /** The crop for a ratio setting and turn: the largest of the ratio around the crop's centre. */
  const refit = (state: RatioState, rot90 = geometry.rot90): CropRect => {
    const wanted = fractionsFor(state, rot90);
    return wanted === null ? geometry.crop : fitLargest(geometry.crop, wanted);
  };
  const chooseRatio = (next: RatioState) => {
    onRatioChange(next);
    const crop = refit(next);
    if (!sameCrop(crop, geometry.crop)) change({ crop });
  };
  const turnTo = (rot90: number) => change({ rot90, crop: refit(ratio, rot90) });
  /** With a lock, the exact crops below become the largest of the ratio inside them. */
  const detectedCrop = detected ? (k ? fitInside(detected.rect, k) : detected.rect) : FULL_CROP;
  const wholeCrop = k ? fitInside(FULL_CROP, k) : FULL_CROP;
  const ratioText = (() => {
    if (k === null || !scan) return '';
    const finished = finishedRatio(ratio, scan, geometry.rot90) ?? 1;
    return PRESET_MODES.includes(ratio.mode as (typeof PRESET_MODES)[number]) &&
      ratio.mode !== 'scan'
      ? ratio.portrait
        ? ratio.mode.split(':').reverse().join(':')
        : ratio.mode
      : t('develop.crop.ratio.decimal', {
          value: new Intl.NumberFormat(i18n.language, {
            minimumFractionDigits: 2,
            maximumFractionDigits: 2,
          }).format(finished),
        });
  })();
  const customInvalid = ratio.mode === 'custom' && customRatio(ratio.custom) === null;

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
            disabled={locked || !confident || sameCrop(geometry.crop, detectedCrop)}
            onClick={() => change({ crop: detectedCrop })}
          >
            {t('develop.crop.useDetected')}
          </button>
          <button
            type="button"
            disabled={locked || sameCrop(geometry.crop, wholeCrop)}
            onClick={() => change({ crop: wholeCrop })}
          >
            {t('develop.crop.useWhole')}
          </button>
        </div>
        {cropping && (
          <p className={styles.panelText} role="status">
            {cropReady ? t('develop.crop.editing') : t('develop.crop.preparing')}
          </p>
        )}
        <div className={styles.ratio}>
          <label className={styles.ratioField}>
            {t('develop.crop.ratio.label')}
            <select
              value={ratio.mode}
              disabled={locked || !scan}
              onChange={(event) =>
                chooseRatio({ ...ratio, mode: event.target.value as RatioMode, portrait: false })
              }
            >
              <option value="free">{t('develop.crop.ratio.free')}</option>
              <option value="scan">{t('develop.crop.ratio.scan')}</option>
              {PRESET_MODES.filter((mode) => mode !== 'scan').map((mode) => (
                <option key={mode} value={mode}>
                  {mode}
                </option>
              ))}
              <option value="custom">{t('develop.crop.ratio.custom')}</option>
            </select>
          </label>
          {ratio.mode === 'custom' && (
            <fieldset className={styles.ratioPair}>
              <legend>{t('develop.crop.ratio.customLegend')}</legend>
              {(['w', 'h'] as const).map((side) => (
                <label key={side} className={styles.ratioField}>
                  {t(`develop.crop.ratio.${side === 'w' ? 'width' : 'height'}`)}
                  <input
                    type="text"
                    inputMode="decimal"
                    value={ratio.custom[side]}
                    disabled={locked}
                    aria-invalid={customInvalid}
                    onChange={(event) =>
                      chooseRatio({
                        ...ratio,
                        custom: { ...ratio.custom, [side]: event.target.value },
                      })
                    }
                  />
                </label>
              ))}
            </fieldset>
          )}
          {customInvalid && (
            <p className={styles.panelText} role="alert">
              {t('develop.crop.ratio.invalid')}
            </p>
          )}
          {ratio.mode !== 'free' && (
            <div className={styles.panelButtons}>
              <button
                type="button"
                disabled={locked || k === null}
                onClick={() => chooseRatio({ ...ratio, portrait: !ratio.portrait })}
              >
                {t('develop.crop.ratio.swap')}
              </button>
              <button
                type="button"
                disabled={locked || k === null || sameCrop(geometry.crop, refit(ratio))}
                onClick={() => change({ crop: refit(ratio) })}
              >
                {t('develop.crop.ratio.fit')}
              </button>
            </div>
          )}
          {k !== null && (
            <p className={styles.panelText}>
              {t('develop.crop.ratio.locked', { ratio: ratioText })}
            </p>
          )}
        </div>
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
                  change(
                    {
                      crop: k
                        ? moveEdgeLocked(geometry.crop, edge, next, k)
                        : moveEdge(geometry.crop, edge, next),
                    },
                    dragging,
                  )
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
          <button type="button" disabled={locked} onClick={() => turnTo((geometry.rot90 + 3) % 4)}>
            {t('develop.geometry.rotateLeft')}
          </button>
          <button type="button" disabled={locked} onClick={() => turnTo((geometry.rot90 + 1) % 4)}>
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
            onClick={() =>
              change({
                rot90: 0,
                flip_h: false,
                flip_v: false,
                angle_deg: 0,
                crop: refit(ratio, 0),
              })
            }
          >
            {t('develop.geometry.reset')}
          </button>
        </div>
      </section>
    </div>
  );
}
