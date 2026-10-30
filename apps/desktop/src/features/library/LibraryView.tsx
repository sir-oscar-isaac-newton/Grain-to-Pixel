import { useTranslation } from 'react-i18next';

export function LibraryView() {
  const { t } = useTranslation();
  return (
    <section aria-labelledby="library-title">
      <h2 id="library-title">{t('library.title')}</h2>
      <p>{t('library.placeholder')}</p>
    </section>
  );
}
