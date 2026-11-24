import { StatusBadge } from '@gtp/ui';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { frameList, type FrameSummary } from '../../../../shared/ipc';
import { useLibraryStore } from '../../../library/library-store';
import { useThumbnail } from '../../../library/use-thumbnail';
import styles from '../../import.module.css';
import { useCaptureRig } from '../CaptureRig';
import { NextFrameButton } from '../NextFrameButton';
import { StepSection } from './StepSection';

/** How many of the newest frames the strip shows. */
const STRIP = 12;

interface Shown {
  total: number;
  frames: FrameSummary[];
}

/**
 * The newest frames of the roll, at most `STRIP` of them, and how many it has. The list is in roll
 * order a page at a time, so the newest frames are the end of the last two pages.
 */
async function newest(rollId: string): Promise<Shown> {
  const first = await frameList({ roll_id: rollId, page: 0, page_size: STRIP });
  const last = Math.max(0, Math.ceil(first.total / STRIP) - 1);
  if (last === 0) return { total: first.total, frames: first.items };
  const [before, end] = await Promise.all([
    last - 1 === 0 ? first : frameList({ roll_id: rollId, page: last - 1, page_size: STRIP }),
    frameList({ roll_id: rollId, page: last, page_size: STRIP }),
  ]);
  return { total: end.total, frames: [...before.items, ...end.items].slice(-STRIP) };
}

function FrameThumb({ frame, number }: { frame: FrameSummary; number: number }) {
  const { t } = useTranslation();
  const thumbnail = useThumbnail(frame.id, frame.has_thumb);
  const label = t('import.steps.capture.frame', { number });
  if (thumbnail.status === 'ready') {
    return (
      <img src={thumbnail.url} alt={label} draggable={false} className={styles.captureThumb} />
    );
  }
  return <StatusBadge tone="neutral">{label}</StatusBadge>;
}

/** Step "Capture": the Next frame button, how many frames the roll has and the newest ones. */
export function CaptureStep() {
  const { t } = useTranslation();
  const rig = useCaptureRig();
  // The frames are asked for again each time a capture finishes.
  const captured = useLibraryStore(
    (s) => s.jobs.filter((j) => j.kind === 'capture' && j.state === 'done').length,
  );
  const [shown, setShown] = useState<Shown | null>(null);

  useEffect(() => {
    let alive = true;
    newest(rig.rollId).then(
      (found) => {
        if (alive) setShown(found);
      },
      () => {
        // The counter and the strip stay as they were; the frames themselves are stored.
      },
    );
    return () => {
      alive = false;
    };
  }, [rig.rollId, captured]);

  return (
    <StepSection id="import-capture" heading={t('import.steps.capture.heading')}>
      <p className={styles.muted}>{t('import.steps.capture.intro')}</p>
      <NextFrameButton
        ready={rig.usable && !rig.settingsChanging && !rig.running}
        roll={rig.roll}
        levels={rig.levels}
        onBusyChange={rig.setCapturing}
      />
      {shown && (
        <>
          {/* Not a live region: Next frame already announces each new frame. */}
          <p>{t('import.steps.capture.count', { count: shown.total })}</p>
          {shown.frames.length > 0 && (
            <ul aria-label={t('import.steps.capture.strip')} className={styles.frameStrip}>
              {shown.frames.map((frame, i) => (
                <li key={frame.id}>
                  <FrameThumb
                    frame={frame}
                    number={frame.index_in_roll ?? shown.total - shown.frames.length + i + 1}
                  />
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </StepSection>
  );
}
