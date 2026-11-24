import { useTranslation } from 'react-i18next';
import styles from '../../import.module.css';
import { StepSection } from '../../capture/steps/StepSection';
import { DPIS, SAMPLES } from '../frames';
import { useScan } from '../ScanProvider';

/** Step "Settings": the resolution, the samples per pixel and whether the infrared is scanned. */
export function ScanSettingsStep() {
  const { t } = useTranslation();
  const scan = useScan();
  return (
    <StepSection id="import-scan-settings" heading={t('import.steps.scanSettings.heading')}>
      <p className={styles.muted}>{t('import.steps.scanSettings.intro')}</p>
      <div className={styles.row}>
        <label className={styles.field}>
          {t('import.scanner.dpi')}
          <select
            value={scan.dpi}
            disabled={scan.locked}
            onChange={(e) => scan.setDpi(Number(e.target.value))}
          >
            {DPIS.map((d) => (
              <option key={d} value={d}>
                {t('import.scanner.dpiOption', { dpi: d })}
              </option>
            ))}
          </select>
        </label>
        <label className={styles.field}>
          {t('import.scanner.samples')}
          <select
            value={scan.samples}
            disabled={scan.locked}
            onChange={(e) => scan.setSamples(Number(e.target.value))}
          >
            {SAMPLES.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </label>
        <label>
          <input
            type="checkbox"
            checked={scan.ir}
            disabled={scan.locked}
            onChange={(e) => scan.setIr(e.target.checked)}
          />{' '}
          {t('import.scanner.ir')}
        </label>
      </div>
    </StepSection>
  );
}
