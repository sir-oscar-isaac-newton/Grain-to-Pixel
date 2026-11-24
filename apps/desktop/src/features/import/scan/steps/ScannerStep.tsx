import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import styles from '../../import.module.css';
import { StepSection } from '../../capture/steps/StepSection';
import { useScan } from '../ScanProvider';

/** Step "Scanner": find the scanner, or say why there is none. */
export function ScannerStep() {
  const { t } = useTranslation();
  const scan = useScan();
  const { ensureDevices, devices, device, deviceError } = scan;
  useEffect(() => {
    ensureDevices();
  }, [ensureDevices]);

  return (
    <StepSection id="import-scanner" heading={t('import.steps.scanner.heading')}>
      <p className={styles.muted}>{t('import.steps.scanner.intro')}</p>
      {devices === null ? (
        <p role="status">{t('import.scanner.looking')}</p>
      ) : device ? (
        <p>
          {t('import.scanner.device', { name: device.name })}{' '}
          <button type="button" disabled={scan.scanning} onClick={scan.refreshDevices}>
            {t('import.scanner.refresh')}
          </button>
        </p>
      ) : (
        <div>
          <p role="alert">{deviceError ?? t('import.scanner.none')}</p>
          <p className={styles.muted}>{t('import.scanner.driverHint')}</p>
          <button type="button" onClick={scan.refreshDevices}>
            {t('import.scanner.refresh')}
          </button>
        </div>
      )}
    </StepSection>
  );
}
