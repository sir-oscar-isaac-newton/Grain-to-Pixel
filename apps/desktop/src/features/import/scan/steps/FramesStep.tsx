import { useEffect, useId, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import styles from '../../import.module.css';
import { StepSection } from '../../capture/steps/StepSection';
import { jpegUrl, MAX_FRAMES } from '../frames';
import { useScan } from '../ScanProvider';

/** Step "Frames": the roll preview, the first and last frame to scan, and where scanning continues. */
export function FramesStep() {
  const { t } = useTranslation();
  const id = useId();
  const scan = useScan();
  const { ensureDevices, problems, preview } = scan;
  useEffect(() => {
    ensureDevices();
  }, [ensureDevices]);

  return (
    <StepSection id="import-frames" heading={t('import.steps.frames.heading')}>
      <p className={styles.muted}>{t('import.steps.frames.intro')}</p>

      <p className={styles.muted}>
        {scan.complete
          ? t('import.scanner.complete')
          : scan.resume > 1
            ? t('import.scanner.resume', { n: scan.resume })
            : t('import.scanner.fresh')}{' '}
        {scan.resume > 1 && (
          <button type="button" disabled={scan.locked} onClick={() => scan.startFrom(1)}>
            {t('import.scanner.fromStart')}
          </button>
        )}
      </p>

      <div className={styles.row}>
        <label className={styles.field}>
          {t('import.scanner.first')}
          <input
            type="number"
            inputMode="numeric"
            min={1}
            max={MAX_FRAMES}
            value={scan.first}
            disabled={scan.locked}
            aria-invalid={problems.first ? true : undefined}
            aria-describedby={problems.first ? `${id}-first-problem` : undefined}
            onChange={(e) => scan.setFirst(e.target.value)}
          />
        </label>
        <label className={styles.field}>
          {t('import.scanner.last')}
          <input
            type="number"
            inputMode="numeric"
            min={1}
            max={MAX_FRAMES}
            value={scan.last}
            disabled={scan.locked}
            aria-invalid={problems.last ? true : undefined}
            aria-describedby={problems.last ? `${id}-last-problem` : undefined}
            onChange={(e) => scan.setLast(e.target.value)}
          />
        </label>
      </div>
      {problems.first && (
        <p id={`${id}-first-problem`} className={styles.problem}>
          {t(problems.first, { max: MAX_FRAMES })}
        </p>
      )}
      {problems.last && (
        <p id={`${id}-last-problem`} className={styles.problem}>
          {t(problems.last, { max: MAX_FRAMES })}
        </p>
      )}

      <div className={styles.buttons}>
        <button
          type="button"
          disabled={scan.locked || scan.previewing || !scan.device}
          onClick={() => void scan.previewRoll()}
        >
          {scan.previewing ? t('import.scanner.previewing') : t('import.scanner.preview')}
        </button>
        {scan.previewing && (
          <button type="button" onClick={scan.cancelPreview}>
            {t('import.scanner.previewCancel')}
          </button>
        )}
      </div>

      {scan.previewError && <p role="alert">{scan.previewError}</p>}

      {preview && (
        <figure className={styles.preview}>
          <div className={styles.previewScroll}>
            <div
              className={styles.previewStrip}
              style={{ '--pitch': `${preview.pitch}px` } as CSSProperties}
            >
              <img
                src={jpegUrl(preview.strip_jpeg)}
                width={preview.strip_width}
                height={preview.strip_height}
                alt={t('import.scanner.previewAlt', { count: preview.frame_count })}
              />
            </div>
          </div>
          <figcaption className={styles.muted}>
            {t('import.scanner.previewCaption', { count: preview.frame_count })}
          </figcaption>
        </figure>
      )}
    </StepSection>
  );
}
