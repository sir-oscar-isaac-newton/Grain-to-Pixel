import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import './shared/i18n';
import './shared/styles/tokens.css';
import './shared/styles/base.css';

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
