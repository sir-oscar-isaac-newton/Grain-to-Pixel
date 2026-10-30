import { useTranslation } from 'react-i18next';

export function DevelopView() {
  const { t } = useTranslation();
  return (
    <section aria-labelledby="develop-title">
      <h2 id="develop-title">{t('develop.title')}</h2>
      <p>{t('develop.placeholder')}</p>
    </section>
  );
}
