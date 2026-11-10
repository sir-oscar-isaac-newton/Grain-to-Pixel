import { useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Canvas } from './Canvas';
import styles from './develop.module.css';
import { edgeFor, useReferenceRender } from './use-compare';
import { fitZoom, type Size, type View } from './viewport';

interface ComparePaneProps {
  /** The reference frame. */
  id: string;
  /** The frame's name in words, such as "Frame 3". */
  frameLabel: string;
  /** The Develop view's counter for render sequence numbers. */
  nextSeq: () => number;
}

/**
 * The reference frame next to the one being edited: read-only, drawn with its own saved recipe,
 * and always fitted to the pane (no zoom, no pan). It is drawn again only when the reference or
 * the size of the pane changes.
 */
export function ComparePane({ id, frameLabel, nextSeq }: ComparePaneProps) {
  const { t } = useTranslation();
  const stageRef = useRef<HTMLDivElement>(null);
  const [stage, setStage] = useState<Size>({ width: 0, height: 0 });

  useLayoutEffect(() => {
    const element = stageRef.current;
    if (!element) return;
    const read = () => setStage({ width: element.clientWidth, height: element.clientHeight });
    read();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(read);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const edge = edgeFor(stage.width, stage.height);
  const shown = useReferenceRender(id, edge, nextSeq);
  const picture = shown.id === id ? shown.preview : null;
  const status = shown.id === id ? shown.status : 'loading';
  const view = useMemo<View | null>(
    () => (picture ? { fit: true, zoom: fitZoom(stage, picture), offset: { x: 0, y: 0 } } : null),
    [picture, stage],
  );

  return (
    <div className={styles.pane}>
      <p className={styles.paneCaption}>{t('develop.compare.reference', { frame: frameLabel })}</p>
      <div ref={stageRef} className={styles.stage} aria-busy={status === 'loading'}>
        {picture && view && (
          <Canvas
            preview={picture}
            label={t('develop.compare.referenceLabel', { frame: frameLabel })}
            view={view}
          />
        )}
        {status === 'loading' && (
          <p role="status" className={styles.message}>
            {t('develop.compare.loading')}
          </p>
        )}
        {shown.id === id && shown.status === 'error' && (
          <p role="alert" className={styles.message}>
            {t('develop.compare.failed', { message: shown.error })}
          </p>
        )}
      </div>
    </div>
  );
}
