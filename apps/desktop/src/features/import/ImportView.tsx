import { useTranslation } from 'react-i18next';

export function ImportView() {
  const { t } = useTranslation();
  return (
    <section aria-labelledby="import-title">
      <h2 id="import-title">{t('import.title')}</h2>
      <p>{t('import.placeholder')}</p>
    </section>
  );
}
