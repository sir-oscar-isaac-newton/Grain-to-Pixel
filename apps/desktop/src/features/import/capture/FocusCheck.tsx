import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  captureFocusCheck,
  settingsGet,
  settingsSet,
  type FocusCheck as Check,
  type FocusMap,
  type FocusPattern,
  type Levels,
  type TripletChannel,
} from '../../../shared/ipc';
import { messageOf } from '../../library/library-store';
import styles from '../import.module.css';
import { formatExposure } from './exposure';

const CHANNELS = ['red', 'green', 'blue'] as const;
const ROWS = ['top', 'middle', 'bottom'] as const;
const COLUMNS = ['left', 'centre', 'right'] as const;

interface FocusCheckProps {
  /** True while a capture runs, or when there is no camera or light to shoot with. */
  disabled: boolean;
  /** The working levels the shot is taken at. */
  levels: Levels;
  /** Called with true when a check starts and false when it ends. */
  onBusyChange: (busy: boolean) => void;
}

const percent = (value: number) => `${Math.round(value * 100)} %`;

const oneDecimal = (value: number) =>
  value.toLocaleString(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 1 });

/** The key of the sentence that names what a pattern shows. */
function patternKey(pattern: FocusPattern): string {
  return pattern.kind === 'tilted' ? 'tilted' : pattern.kind;
}

/**
 * One real picture in one light colour: a 1:1 crop of the middle with its sharpness number, and a
 * 3 x 3 map of sharpness that shows whether the film lies flat. Nothing is stored; the map of a
 * flat target can be saved as the reference that later maps are shown against.
 */
export function FocusCheck({ disabled, levels, onBusyChange }: FocusCheckProps) {
  const { t } = useTranslation();
  const [channel, setChannel] = useState<TripletChannel>('green');
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<Check | null>(null);
  const [best, setBest] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reference, setReference] = useState<FocusMap | null>(null);
  const [saving, setSaving] = useState(false);

  const onBusyChangeRef = useRef(onBusyChange);
  onBusyChangeRef.current = onBusyChange;
  useEffect(() => {
    onBusyChangeRef.current(busy);
  }, [busy]);

  // The saved flat reference, if there is one.
  useEffect(() => {
    let alive = true;
    settingsGet().then(
      (settings) => {
        if (alive) setReference(settings.focus_reference ?? null);
      },
      () => {
        // Unreadable settings: no reference is shown.
      },
    );
    return () => {
      alive = false;
    };
  }, []);

  const check = () => {
    setBusy(true);
    setError(null);
    captureFocusCheck(channel, levels)
      .then(
        (found) => {
          setResult(found);
          const value = found.sharpness;
          if (value !== null) setBest((b) => (b === null || value > b ? value : b));
        },
        (e: unknown) => setError(messageOf(e)),
      )
      .finally(() => setBusy(false));
  };

  const saveReference = async (patch: Parameters<typeof settingsSet>[0]) => {
    setSaving(true);
    setError(null);
    try {
      const settings = await settingsSet(patch);
      setReference(settings.focus_reference ?? null);
    } catch (e) {
      setError(messageOf(e));
    } finally {
      setSaving(false);
    }
  };

  const aperture = result
    ? formatExposure({ exposure_time: null, iso: null, f_number: result.exposure.f_number })
    : null;

  return (
    <div className={styles.captureTest}>
      <fieldset className={styles.captureLiveLight} disabled={disabled || busy}>
        <legend>{t('import.capture.focusCheck.heading')}</legend>
        {CHANNELS.map((choice) => (
          <label key={choice}>
            <input
              type="radio"
              name="focus-check-channel"
              checked={channel === choice}
              onChange={() => setChannel(choice)}
            />
            {t(`import.capture.focusCheck.channel.${choice}`)}
          </label>
        ))}
      </fieldset>
      <div className={styles.buttons}>
        <button
          type="button"
          disabled={disabled || busy}
          aria-describedby="import-focus-check-help"
          onClick={check}
        >
          {busy ? t('import.capture.focusCheck.running') : t('import.capture.focusCheck.label')}
        </button>
      </div>
      <p id="import-focus-check-help" className={styles.muted}>
        {t('import.capture.focusCheck.help')}
      </p>
      {busy && <p role="status">{t('import.capture.focusCheck.taking')}</p>}
      {error && <p role="alert">{error}</p>}
      {result && (
        <div className={styles.captureFocusResult}>
          <div className={styles.captureFocusCrop}>
            <img
              src={`data:image/jpeg;base64,${result.crop}`}
              width={result.crop_width}
              height={result.crop_height}
              alt={t('import.capture.focusCheck.cropAlt', {
                channel: t(`import.channel.${channel}`),
              })}
            />
          </div>
          <p role="status">
            {result.sharpness === null
              ? t('import.capture.focusCheck.sharpnessNone')
              : t('import.capture.focusCheck.sharpness', { value: oneDecimal(result.sharpness) })}
            {best !== null &&
              ` ${t('import.capture.focusCheck.best', { value: oneDecimal(best) })}`}
          </p>
          <div className={styles.buttons}>
            <button type="button" disabled={best === null} onClick={() => setBest(null)}>
              {t('import.capture.focusCheck.resetBest')}
            </button>
          </div>
          <table className={styles.captureFocusMap}>
            <caption>
              {result.against_reference
                ? t('import.capture.focusCheck.mapAgainst')
                : t('import.capture.focusCheck.map')}
            </caption>
            <thead>
              <tr>
                <td />
                {COLUMNS.map((column) => (
                  <th key={column} scope="col">
                    {t(`import.capture.focusCheck.column.${column}`)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {ROWS.map((row, r) => (
                <tr key={row}>
                  <th scope="row">{t(`import.capture.focusCheck.row.${row}`)}</th>
                  {COLUMNS.map((column, c) => {
                    const value = result.map[r]?.[c] ?? null;
                    return (
                      <td
                        key={column}
                        style={{
                          background:
                            value === null ? undefined : `hsl(120 45% ${96 - value * 40}%)`,
                          color: '#111',
                        }}
                      >
                        {value === null ? '–' : percent(value)}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
          {result.against_reference && (
            <p role="status">{t('import.capture.focusCheck.againstReference')}</p>
          )}
          <p role="status">
            {t(`import.capture.focusCheck.pattern.${patternKey(result.pattern)}`, {
              side:
                result.pattern.kind === 'tilted'
                  ? t(`import.capture.focusCheck.side.${result.pattern.side}`)
                  : '',
            })}
          </p>
          <p className={styles.muted}>
            {t('import.capture.focusCheck.caveat')}
            {aperture && ` ${t('import.capture.focusCheck.aperture', { aperture })}`}
          </p>
          <div className={styles.buttons}>
            <button
              type="button"
              disabled={saving}
              onClick={() => void saveReference({ focus_reference: result.raw_map })}
            >
              {t('import.capture.focusCheck.saveReference')}
            </button>
          </div>
        </div>
      )}
      {reference && (
        <div className={styles.buttons}>
          <p role="status">{t('import.capture.focusCheck.referenceSaved')}</p>
          <button
            type="button"
            disabled={saving}
            onClick={() => void saveReference({ clear_focus_reference: true })}
          >
            {t('import.capture.focusCheck.clearReference')}
          </button>
        </div>
      )}
    </div>
  );
}
