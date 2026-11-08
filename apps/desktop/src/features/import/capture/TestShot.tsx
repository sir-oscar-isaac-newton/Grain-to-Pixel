import { Histogram } from '@gtp/ui';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  captureTest,
  type ExposureInfo,
  type Levels,
  type TestShot as Shot,
  type TripletChannel,
} from '../../../shared/ipc';
import { messageOf } from '../../library/library-store';
import styles from '../import.module.css';

const CHANNELS = ['red', 'green', 'blue'] as const;

/** More than this share of clipped photosites (in percent) is warned about (spec section 6.1). */
export const CLIP_WARN_PCT = 0.1;

const NONE = new Array<number>(256).fill(0);

interface TestShotProps {
  /** True while a capture runs, or when there is no camera or light to shoot with. */
  disabled: boolean;
  /** The working levels the test shot is taken at. */
  levels: Levels;
  /** Called with the camera settings of each test picture that was taken. */
  onShot: (exposure: ExposureInfo) => void;
}

/** One test picture per colour: its histogram and how much of it is clipped. Nothing is stored. */
export function TestShot({ disabled, levels, onShot }: TestShotProps) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState<TripletChannel | null>(null);
  const [shown, setShown] = useState<{ channel: TripletChannel; shot: Shot } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const shoot = (channel: TripletChannel) => {
    setBusy(channel);
    setError(null);
    captureTest(channel, levels)
      .then(
        (shot) => {
          setShown({ channel, shot });
          onShot(shot.exposure);
        },
        (e: unknown) => setError(messageOf(e)),
      )
      .finally(() => setBusy(null));
  };

  const clipped = shown?.shot.clipped_pct ?? null;
  // The lit channel in its own colour; the others stay empty.
  const histogram = shown
    ? { red: NONE, green: NONE, blue: NONE, luma: NONE, [shown.channel]: shown.shot.histogram }
    : null;

  return (
    <div className={styles.captureTest}>
      <div className={styles.buttons}>
        {CHANNELS.map((channel) => (
          <button
            key={channel}
            type="button"
            disabled={disabled || busy !== null}
            onClick={() => shoot(channel)}
          >
            {busy === channel
              ? t('import.capture.test.shooting')
              : t('import.capture.test.shoot', { channel: t(`import.channel.${channel}`) })}
          </button>
        ))}
      </div>
      {error && <p role="alert">{error}</p>}
      {shown && histogram && (
        <Histogram
          data={histogram}
          label={t('import.capture.test.histogram', {
            channel: t(`import.channel.${shown.channel}`),
          })}
          caption={
            clipped === null
              ? t('import.capture.test.clippedUnknown')
              : t('import.capture.test.clipped', { pct: clipped.toFixed(2) })
          }
        />
      )}
      {shown && clipped !== null && clipped > CLIP_WARN_PCT && (
        <p role="alert">
          {t('import.capture.test.clipWarning', {
            channel: t(`import.channel.${shown.channel}`),
            pct: clipped.toFixed(2),
          })}
        </p>
      )}
    </div>
  );
}
