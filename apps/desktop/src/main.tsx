import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { ErrorBoundary } from './shared/errors/ErrorBoundary';
import { installGlobalErrorHandlers } from './shared/errors/globalHandlers';
import './shared/i18n';
import './shared/styles/tokens.css';
import './shared/styles/base.css';

installGlobalErrorHandlers();

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </StrictMode>,
  );
}
