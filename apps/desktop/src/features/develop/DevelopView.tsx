import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Histogram } from '@gtp/ui';
import { useUiStore, type DevelopFrame } from '../../app/store';
import {
  developAnalyze,
  developSampleBase,
  developSampleLevels,
  developSampleNeutral,
  frameGet,
  rollSetFilmBase,
  type FrameSummary,
  type LevelsPoint,
} from '../../shared/ipc';
import {
  COLOUR_ACTIONS,
  COLOUR_FINE_STEP,
  COLOUR_STEP,
  actionForKey,
  type ColourAxis,
} from '../../shared/keymap';
import { useLibraryStore } from '../library/library-store';
import { useFrames } from '../library/use-frames';
import { ApplySettingsDialog } from '../library/ApplySettingsDialog';
import { ExportDialog } from '../library/ExportDialog';
import { useLibraryEvents } from '../library/use-library-events';
import { useLibraryQuery } from '../library/use-library-query';
import { Canvas, pictureStyle } from './Canvas';
import { CropOverlay } from './CropOverlay';
import styles from './develop.module.css';
import { Filmstrip } from './Filmstrip';
import { clipping, computeHistogram } from './histogram';
import { BaseInvertPanel } from './panels/BaseInvertPanel';
import { ColourPanel } from './panels/ColourPanel';
import { GeometryPanel } from './panels/GeometryPanel';
import { TonePanel } from './panels/TonePanel';
import type { Fractions } from './picker';
import {
  baseOf,
  colourOf,
  levelsIn,
  levelsOf,
  triple,
  withBase,
  geometryOf,
  withColour,
  withGeometry,
  withLevels,
} from './recipe-parts';
import { useDevelop } from './use-develop';
import { useKeyMap } from './use-keymap';
import { usePicker } from './use-picker';
import { useViewport } from './use-viewport';

/** What is being picked on the picture: the film base, the white or black point, or a neutral grey. */
type PickTarget = 'base' | 'neutral' | 'crop' | LevelsPoint;

interface Notice {
  tone: 'info' | 'error';
  text: string;
}

const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * The Develop view: the open frame on a canvas with pan and zoom, a histogram of what is shown, a
 * before/after switch, and the filmstrip of the frames the Library was showing. The recipe panels
 * come with the issues that follow.
 */
export function DevelopView() {
  const { t } = useTranslation();
  const frame = useUiStore((s) => s.developFrame);
  const setView = useUiStore((s) => s.setView);
  return (
    <section aria-labelledby="develop-title" className={styles.view}>
      <h2 id="develop-title" className={styles.title}>
        {t('develop.title')}
      </h2>
      {frame ? (
        <OpenFrame frame={frame} />
      ) : (
        <div className={styles.empty}>
          <p>{t('develop.noFrame')}</p>
          <button type="button" onClick={() => setView('library')}>
            {t('develop.toLibrary')}
          </button>
        </div>
      )}
    </section>
  );
}

const percent = (language: string, share: number) =>
  new Intl.NumberFormat(language, { style: 'percent', maximumFractionDigits: 1 }).format(share);

function OpenFrame({ frame }: { frame: DevelopFrame }) {
  const { t, i18n } = useTranslation();
  const setView = useUiStore((s) => s.setView);
  const openInDevelop = useUiStore((s) => s.openInDevelop);
  const revision = useLibraryStore((s) => s.revision);
  const query = useLibraryQuery();
  const frames = useFrames(query.fetchPage, query.queryKey, revision);
  const develop = useDevelop(frame.id);
  const { preview, recipe, info } = develop;
  const readOnly = useLibraryStore((s) => s.library?.read_only ?? false);
  // Frames edited here change on disk, so the filmstrip's thumbnails follow the events.
  useLibraryEvents(true);

  const [pick, setPick] = useState<PickTarget | null>(null);
  const [exporting, setExporting] = useState(false);
  const [applying, setApplying] = useState(false);
  const selection = useLibraryStore((s) => s.selected);
  const [guide, setGuide] = useState(false);
  const [working, setWorking] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const viewport = useViewport(
    preview ? { width: preview.width, height: preview.height } : null,
    pick === null,
  );
  const { toggleBefore, setPickView, setRecipe, rerender } = develop;
  const { fit, actual, zoomIn, zoomOut } = viewport;

  // A new frame starts fitted to the stage, with nothing being picked.
  useEffect(() => {
    fit();
    setPick(null);
    setNotice(null);
  }, [frame.id, fit]);

  // Picking the base shows the whole scan (the rebate is outside the crop); picking the white or
  // black point shows the frame without turns, so a click is a position in the cropped frame. The
  // picture goes back afterwards.
  const view = pick === null ? null : pick === 'base' || pick === 'crop' ? 'scan' : 'frame';
  useEffect(() => {
    setPickView(view);
    fit();
  }, [view, setPickView, fit]);

  // The crop is edited without drawing the edit (the picture on screen is the whole scan); when
  // that ends, the edit is drawn once.
  const wasCropping = useRef(false);
  useEffect(() => {
    if (wasCropping.current && pick !== 'crop') rerender();
    wasCropping.current = pick === 'crop';
  }, [pick, rerender]);

  const summary = frames.frameAt(frame.index);
  const label = t('library.frameLabel', {
    n: summary?.id === frame.id ? summary.index_in_roll : frame.index + 1,
  });

  /** A click or box on the whole scan: the film base of this frame becomes what is under it. */
  const picked = async (region: Fractions) => {
    if (!recipe) return;
    try {
      const rgb = await developSampleBase(frame.id, region);
      setRecipe(withBase(recipe, { source: 'manual', rgb: triple(rgb, 1) }));
      setPick(null);
      setNotice({ tone: 'info', text: t('develop.base.picked') });
    } catch (error) {
      setNotice({
        tone: 'error',
        text: t('develop.base.pickFailed', { message: messageOf(error) }),
      });
    }
  };
  /** A click or box on the frame: the colour axes are set so that what is under it is grey. */
  const pickedNeutral = async (region: Fractions) => {
    if (!recipe) return;
    try {
      const color = await developSampleNeutral(frame.id, recipe, region);
      setRecipe(
        withColour(recipe, {
          red_cyan: color.red_cyan ?? 0,
          green_magenta: color.green_magenta ?? 0,
          blue_yellow: color.blue_yellow ?? 0,
        }),
      );
      setPick(null);
      setNotice({ tone: 'info', text: t('develop.colour.picked') });
    } catch (error) {
      setNotice({
        tone: 'error',
        text: t('develop.colour.pickFailed', { message: messageOf(error) }),
      });
    }
  };

  /** A click or box on the frame: the white or black point becomes what is under it. */
  const pickedLevel = async (region: Fractions, point: LevelsPoint) => {
    if (!recipe) return;
    try {
      const levels = await developSampleLevels(frame.id, recipe, region, point);
      setRecipe(withLevels(recipe, levelsIn(levels)));
      setPick(null);
      setNotice({
        tone: 'info',
        text: t('develop.levels.picked', { kind: t(`develop.levels.${point}`).toLowerCase() }),
      });
    } catch (error) {
      setNotice({
        tone: 'error',
        text: t('develop.levels.pickFailed', { message: messageOf(error) }),
      });
    }
  };
  const pickBox = usePicker({
    stageRef: viewport.stageRef,
    active:
      pick === 'base'
        ? develop.showingScan
        : pick !== null && pick !== 'crop' && develop.showingFrame,
    onPick: (region) => {
      if (pick === 'base') void picked(region);
      else if (pick === 'neutral') void pickedNeutral(region);
      else if (pick === 'white' || pick === 'black') void pickedLevel(region, pick);
    },
  });

  /** Finds the levels again, for the film base in use, and replaces the black and white points. */
  const autoLevels = async () => {
    if (!recipe) return;
    const base = baseOf(recipe);
    const given =
      base.source === 'manual' ? base.rgb : info?.roll_base ? triple(info.roll_base, 1) : null;
    setWorking(true);
    try {
      const analysis = await developAnalyze(frame.id, given);
      setRecipe(withLevels(recipe, levelsOf(analysis.auto_recipe)));
      setNotice({ tone: 'info', text: t('develop.levels.autoDone') });
    } catch (error) {
      setNotice({
        tone: 'error',
        text: t('develop.levels.autoFailed', { message: messageOf(error) }),
      });
    } finally {
      setWorking(false);
    }
  };

  /** Makes this frame's picked base the roll's, which the frames that follow the roll then use. */
  const makeRollDefault = async () => {
    if (!recipe) return;
    setWorking(true);
    try {
      const rollId =
        summary?.id === frame.id ? summary.roll_id : (await frameGet(frame.id)).roll_id;
      await rollSetFilmBase(rollId, baseOf(recipe).rgb);
      await develop.refresh();
      setNotice({ tone: 'info', text: t('develop.base.rollDefaultSet') });
    } catch (error) {
      setNotice({
        tone: 'error',
        text: t('develop.base.rollDefaultFailed', { message: messageOf(error) }),
      });
    } finally {
      setWorking(false);
    }
  };

  const open = useCallback(
    (next: FrameSummary, index: number) => openInDevelop(next.id, index),
    [openInDevelop],
  );
  const { total, load } = frames;
  const step = useCallback(
    async (delta: -1 | 1) => {
      const index = frame.index + delta;
      if (index < 0 || index >= total) return;
      const [next] = await load(index, index);
      if (next) open(next, index);
    },
    [frame.index, total, load, open],
  );

  // The keys of the view (the user's own, from the settings). Typing in a field is left alone, and
  // so is anything while a dialog is open: its keys are the dialog's.
  const keyMap = useKeyMap();
  const newest = useRef(recipe);
  newest.current = recipe;
  const repeating = useRef(false);
  useEffect(() => {
    /** A colour key moves its slider; holding it repeats with a lighter preview. */
    const nudgeColour = (move: { axis: ColourAxis; sign: 1 | -1 }, event: KeyboardEvent) => {
      const current = newest.current;
      if (!current || readOnly || working || pick !== null) return;
      const value = colourOf(current)[move.axis];
      const step = event.shiftKey ? COLOUR_FINE_STEP : COLOUR_STEP;
      const next = Math.min(100, Math.max(-100, value + step * move.sign));
      if (next === value) return;
      if (event.repeat) repeating.current = true;
      const changed = withColour(current, { [move.axis]: next });
      newest.current = changed;
      setRecipe(changed, { dragging: event.repeat });
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.ctrlKey || event.metaKey || event.altKey || event.defaultPrevented) return;
      const target = event.target;
      if (
        target instanceof Element &&
        target.closest('input, textarea, select, [contenteditable="true"]')
      )
        return;
      if (document.querySelector('[role="dialog"], [role="alertdialog"]')) return;
      const action = actionForKey(keyMap, event.key);
      if (!action) return;
      event.preventDefault();
      const colour = COLOUR_ACTIONS[action];
      if (colour) {
        nudgeColour(colour, event);
        return;
      }
      switch (action) {
        case 'develop.toggleBefore':
          toggleBefore();
          break;
        case 'develop.previousFrame':
          void step(-1);
          break;
        case 'develop.nextFrame':
          void step(1);
          break;
        case 'develop.backToLibrary':
          // Escape first cancels picking; only then does it leave the view.
          if (pick) setPick(null);
          else setView('library');
          break;
        case 'develop.fit':
          fit();
          break;
        case 'develop.actualSize':
          actual();
          break;
        case 'develop.zoomIn':
          zoomIn();
          break;
        case 'develop.zoomOut':
          zoomOut();
          break;
      }
    };
    // A held colour key drew rough previews; the sharp one is drawn when it is let go.
    const onKeyUp = () => {
      if (!repeating.current) return;
      repeating.current = false;
      rerender();
    };
    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
    };
  }, [
    keyMap,
    toggleBefore,
    step,
    setView,
    fit,
    actual,
    zoomIn,
    zoomOut,
    pick,
    readOnly,
    working,
    setRecipe,
    rerender,
  ]);
  const bins = useMemo(() => (preview ? computeHistogram(preview.rgba) : null), [preview]);
  const lost = bins ? clipping(bins) : null;
  const zoomText = percent(i18n.language, viewport.view.zoom);

  return (
    <div className={styles.frameView}>
      <div className={styles.toolbar} role="toolbar" aria-label={t('develop.toolbar')}>
        <button type="button" onClick={() => setView('library')}>
          {t('develop.toLibrary')}
        </button>
        <span className={styles.frameName}>{label}</span>
        <button
          type="button"
          aria-pressed={develop.showBefore}
          disabled={develop.status !== 'ready'}
          onClick={toggleBefore}
        >
          {t('develop.before')}
        </button>
        <button type="button" onClick={() => setExporting(true)}>
          {t('export.open')}
        </button>
        <button type="button" disabled={readOnly} onClick={() => setApplying(true)}>
          {t('apply.open')}
        </button>
        <span className={styles.spacer} />
        <button type="button" onClick={zoomOut} aria-label={t('develop.zoomOut')}>
          {t('develop.zoomOutText')}
        </button>
        <span className={styles.zoom} aria-label={t('develop.zoomLevel')}>
          {zoomText}
        </span>
        <button type="button" onClick={zoomIn} aria-label={t('develop.zoomIn')}>
          {t('develop.zoomInText')}
        </button>
        <button type="button" onClick={fit} disabled={viewport.fitting}>
          {t('develop.fit')}
        </button>
        <button type="button" onClick={actual}>
          {t('develop.actual')}
        </button>
      </div>

      <div className={styles.body}>
        <div
          ref={viewport.stageRef}
          className={`${styles.stage} ${viewport.fitting || pick !== null ? '' : styles.pannable} ${
            pick !== null && pick !== 'crop' && (develop.showingScan || develop.showingFrame)
              ? styles.picking
              : ''
          }`}
          aria-busy={develop.busy}
        >
          {preview && (
            <Canvas
              preview={preview}
              label={
                develop.showingScan
                  ? pick === 'crop'
                    ? t('develop.cropLabel')
                    : t('develop.base.scanLabel')
                  : develop.showingFrame
                    ? t('develop.pickFrameLabel')
                    : label
              }
              view={viewport.view}
            />
          )}
          {preview && recipe && pick === 'crop' && develop.showingScan && (
            <CropOverlay
              crop={geometryOf(recipe).crop}
              style={pictureStyle(preview, viewport.view)}
              disabled={readOnly || working}
              label={t('develop.crop.area')}
              onChange={(crop, options) =>
                setRecipe(withGeometry(recipe, { crop }), { ...options, preview: false })
              }
            />
          )}
          {preview && guide && pick === null && (
            <div
              className={styles.guide}
              style={pictureStyle(preview, viewport.view)}
              aria-hidden="true"
            />
          )}
          {pickBox && <div className={styles.pickBox} style={pickBox} />}
          {develop.showingBefore && (
            <span className={styles.badge}>{t('develop.beforeBadge')}</span>
          )}
          {develop.status === 'opening' && (
            <p role="status" className={styles.message}>
              {t('develop.opening')}
            </p>
          )}
          {develop.status === 'error' && (
            <p role="alert" className={styles.message}>
              {t('develop.openFailed', { message: develop.error })}
            </p>
          )}
          {develop.status === 'ready' && !preview && (
            <p role="status" className={styles.message}>
              {t('develop.rendering')}
            </p>
          )}
        </div>
        <aside className={styles.side} aria-label={t('develop.side')}>
          <Histogram
            data={bins}
            label={t('develop.histogram.label')}
            caption={
              lost
                ? t('develop.histogram.clipping', {
                    black: percent(i18n.language, lost.black),
                    white: percent(i18n.language, lost.white),
                  })
                : undefined
            }
          />
          <div className={styles.statusArea}>
            <p role="status" className={styles.state}>
              {develop.beforeUnavailable
                ? t('develop.beforeUnavailable')
                : develop.showBefore && !develop.showingBefore
                  ? t('develop.beforeLoading')
                  : develop.showingBefore
                    ? t('develop.showingBefore')
                    : develop.busy && preview
                      ? t('develop.updating')
                      : ''}
            </p>
            {develop.renderError && (
              <p role="alert" className={styles.state}>
                {t('develop.renderFailed', { message: develop.renderError })}
              </p>
            )}
            {notice && (
              <p role={notice.tone === 'error' ? 'alert' : 'status'} className={styles.state}>
                {notice.text}
              </p>
            )}
            {readOnly && <p className={styles.state}>{t('develop.readOnly')}</p>}
            <p role={develop.saveStatus === 'error' ? 'alert' : 'status'} className={styles.state}>
              {develop.saveStatus === 'saving'
                ? t('develop.save.saving')
                : develop.saveStatus === 'saved'
                  ? t('develop.save.saved')
                  : develop.saveStatus === 'error'
                    ? t('develop.save.failed', { message: develop.saveError })
                    : ''}
            </p>
          </div>
          {recipe && (
            <>
              <GeometryPanel
                recipe={recipe}
                readOnly={readOnly}
                working={working}
                cropping={pick === 'crop'}
                cropReady={develop.showingScan}
                onCroppingChange={(on) => setPick(on ? 'crop' : null)}
                detected={develop.detected}
                onGuideChange={setGuide}
                onChange={(next, options) =>
                  setRecipe(next, pick === 'crop' ? { ...options, preview: false } : options)
                }
              />
              <BaseInvertPanel
                recipe={recipe}
                rollBase={info?.roll_base ? triple(info.roll_base, 1) : null}
                readOnly={readOnly}
                pick={pick === 'neutral' || pick === 'crop' ? null : pick}
                pickReady={pick === 'base' ? develop.showingScan : develop.showingFrame}
                working={working}
                onPickChange={setPick}
                onChange={(next, options) => setRecipe(next, options)}
                onAutoLevels={() => void autoLevels()}
                onUseAsRollDefault={() => void makeRollDefault()}
              />
              <ColourPanel
                recipe={recipe}
                readOnly={readOnly}
                working={working}
                picking={pick === 'neutral'}
                pickReady={develop.showingFrame}
                onPickingChange={(on) => setPick(on ? 'neutral' : null)}
                onChange={(next, options) => setRecipe(next, options)}
              />
              <TonePanel
                recipe={recipe}
                readOnly={readOnly}
                working={working}
                onChange={(next, options) => setRecipe(next, options)}
              />
            </>
          )}
        </aside>
      </div>

      {exporting && <ExportDialog frameIds={[frame.id]} open onClose={() => setExporting(false)} />}
      {applying && (
        <ApplySettingsDialog
          open
          onClose={() => setApplying(false)}
          sources={[frame.id]}
          selected={[...selection]}
        />
      )}

      <Filmstrip
        frames={frames}
        currentId={frame.id}
        currentIndex={frame.index}
        onOpen={open}
        onStep={(delta) => void step(delta)}
      />
    </div>
  );
}
