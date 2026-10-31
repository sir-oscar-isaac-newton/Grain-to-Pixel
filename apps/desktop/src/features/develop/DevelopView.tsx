import { useCallback, useEffect, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Histogram } from '@gtp/ui';
import { useUiStore, type DevelopFrame } from '../../app/store';
import type { FrameSummary } from '../../shared/ipc';
import { useLibraryStore } from '../library/library-store';
import { useFrames } from '../library/use-frames';
import { useLibraryQuery } from '../library/use-library-query';
import { Canvas } from './Canvas';
import { actionForKey } from './develop-keys';
import styles from './develop.module.css';
import { Filmstrip } from './Filmstrip';
import { clipping, computeHistogram } from './histogram';
import { useDevelop } from './use-develop';
import { useViewport } from './use-viewport';

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
  const { preview } = develop;
  const viewport = useViewport(preview ? { width: preview.width, height: preview.height } : null);
  const { toggleBefore } = develop;
  const { fit, actual, zoomIn, zoomOut } = viewport;

  // A new frame starts fitted to the stage.
  useEffect(() => {
    fit();
  }, [frame.id, fit]);

  const summary = frames.frameAt(frame.index);
  const label = t('library.frameLabel', {
    n: summary?.id === frame.id ? summary.index_in_roll : frame.index + 1,
  });

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

  // The keys of the view. Typing in a field is left alone.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.ctrlKey || event.metaKey || event.altKey) return;
      const target = event.target;
      if (
        target instanceof Element &&
        target.closest('input, textarea, select, [contenteditable="true"]')
      )
        return;
      const action = actionForKey(event.key);
      if (!action) return;
      event.preventDefault();
      switch (action) {
        case 'toggleBefore':
          toggleBefore();
          break;
        case 'previousFrame':
          void step(-1);
          break;
        case 'nextFrame':
          void step(1);
          break;
        case 'backToLibrary':
          setView('library');
          break;
        case 'fit':
          fit();
          break;
        case 'actualSize':
          actual();
          break;
        case 'zoomIn':
          zoomIn();
          break;
        case 'zoomOut':
          zoomOut();
          break;
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [toggleBefore, step, setView, fit, actual, zoomIn, zoomOut]);

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
          className={`${styles.stage} ${viewport.fitting ? '' : styles.pannable}`}
          aria-busy={develop.busy}
        >
          {preview && <Canvas preview={preview} label={label} view={viewport.view} />}
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
        </aside>
      </div>

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
