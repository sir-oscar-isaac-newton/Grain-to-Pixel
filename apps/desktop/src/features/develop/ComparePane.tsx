import { useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Canvas } from './Canvas';
import styles from './develop.module.css';
import { edgeFor, useReferenceRender, zoomedEdge } from './use-compare';
import { fitZoom, linkedView, type Size, type View } from './viewport';

/** The current frame's view and picture, which the reference follows while the zoom is linked. */
export interface LinkedTo {
  /** The current frame's view as drawn. */
  view: View;
  /** The size of the picture the view is of. */
  image: Size;
}

interface ComparePaneProps {
  /** The reference frame. */
  id: string;
  /** The frame's name in words, such as "Frame 3". */
  frameLabel: string;
  /** The Develop view's counter for render sequence numbers. */
  nextSeq: () => number;
  /** Follow this view (zoom and pan linked), or null to show the reference fitted. */
  link: LinkedTo | null;
  /** Changes when the reference's saved recipe may have changed: it is read again. */
  revision: number;
}

/**
 * The reference frame next to the one being edited: read-only, drawn with its own saved recipe,
 * and taking no pointer input. It is fitted to the pane, or, with the zoom linked, shown at the
 * current frame's zoom and relative place. It is drawn again only when the reference, its recipe,
 * the long edge it needs (from the pane size, or from the zoom while linked) changes; a pan never
 * draws it again.
 */
export function ComparePane({ id, frameLabel, nextSeq, link, revision }: ComparePaneProps) {
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

  const following = link !== null && !link.view.fit;
  const fitEdge = edgeFor(stage.width, stage.height);
  const edge = fitEdge !== null && following ? zoomedEdge(link.view.zoom) : fitEdge;
  const shown = useReferenceRender(id, edge, nextSeq, revision);
  const picture = shown.id === id ? shown.preview : null;
  const status = shown.id === id ? shown.status : 'loading';
  const view = useMemo<View | null>(() => {
    if (!picture) return null;
    if (link) return linkedView(link.view, link.image, stage, picture);
    return { fit: true, zoom: fitZoom(stage, picture), offset: { x: 0, y: 0 } };
  }, [picture, stage, link]);

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
