import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from './en.json';

// English only for the MVP (spec section 13); adding a language means adding a JSON file here.
void i18n.use(initReactI18next).init({
  lng: 'en',
  fallbackLng: 'en',
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

export default i18n;
