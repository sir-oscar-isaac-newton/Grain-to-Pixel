import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  countersGet,
  diagnosticsExport,
  pickDiagnosticsFile,
  type Counters,
} from '../../shared/ipc';
import { messageOf } from '../library/library-store';
import styles from './settings.module.css';

/**
 * The activation counters of the open library and the button that saves the diagnostics zip. The
 * zip goes where the user chooses; nothing is uploaded.
 */
export function DiagnosticsSection() {
  const { t, i18n } = useTranslation();
  const [counters, setCounters] = useState<Counters | null>(null);
  const [countersError, setCountersError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    countersGet().then(
      (found) => {
        if (alive) setCounters(found);
      },
      (e: unknown) => {
        if (alive)
          setCountersError(t('settings.diagnostics.countersFailed', { message: messageOf(e) }));
      },
    );
    return () => {
      alive = false;
    };
  }, [t]);

  const when = (iso: string | null | undefined) =>
    iso
      ? new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' }).format(
          new Date(iso),
        )
      : t('settings.diagnostics.never');

  const save = async () => {
    setSaving(true);
    setSaved(null);
    setError(null);
    try {
      const dest = await pickDiagnosticsFile();
      if (dest === null) return;
      await diagnosticsExport(dest);
      setSaved(t('settings.diagnostics.saved', { path: dest }));
    } catch (e) {
      setError(t('settings.diagnostics.failed', { message: messageOf(e) }));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section aria-labelledby="settings-diagnostics" className={styles.section}>
      <h3 id="settings-diagnostics" className={styles.heading}>
        {t('settings.diagnostics.heading')}
      </h3>
      <p className={styles.muted}>{t('settings.diagnostics.intro')}</p>
      <div className={styles.buttons}>
        <button type="button" disabled={saving} onClick={() => void save()}>
          {saving ? t('settings.diagnostics.exporting') : t('settings.diagnostics.export')}
        </button>
      </div>
      {saved && <p role="status">{saved}</p>}
      {error && <p role="alert">{error}</p>}

      <h4 className={styles.subheading}>{t('settings.diagnostics.countersHeading')}</h4>
      {counters ? (
        <>
          <p className={styles.muted}>{t('settings.diagnostics.countersNote')}</p>
          <dl className={styles.facts}>
            <dt>{t('settings.diagnostics.firstImport')}</dt>
            <dd>{when(counters.first_import_at)}</dd>
            <dt>{t('settings.diagnostics.firstConvert')}</dt>
            <dd>{when(counters.first_convert_at)}</dd>
            <dt>{t('settings.diagnostics.firstExport')}</dt>
            <dd>{when(counters.first_export_at)}</dd>
            <dt>{t('settings.diagnostics.framesImported')}</dt>
            <dd>{counters.frames_imported}</dd>
            <dt>{t('settings.diagnostics.framesExported')}</dt>
            <dd>{counters.frames_exported}</dd>
            <dt>{t('settings.diagnostics.scansFailed')}</dt>
            <dd>{counters.scans_failed}</dd>
          </dl>
        </>
      ) : (
        <p className={styles.muted}>{countersError ?? t('settings.diagnostics.noLibrary')}</p>
      )}
    </section>
  );
}
