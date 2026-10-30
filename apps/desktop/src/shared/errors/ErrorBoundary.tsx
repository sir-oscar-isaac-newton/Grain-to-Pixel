import { Component, type ErrorInfo, type ReactNode } from 'react';
import { withTranslation, type WithTranslation } from 'react-i18next';
import { logClientEvent } from '../ipc';

interface Props extends WithTranslation {
  children: ReactNode;
}

interface State {
  failed: boolean;
}

/** Catches render errors, logs them and shows a recoverable screen without technical text. */
class Boundary extends Component<Props, State> {
  override state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    void logClientEvent(
      'Error',
      `render error: ${error.name}: ${error.message}${info.componentStack ?? ''}`,
    );
  }

  override render(): ReactNode {
    const { t, children } = this.props;
    if (!this.state.failed) return children;
    return (
      <div role="alert">
        <h2>{t('error.title')}</h2>
        <p>{t('error.body')}</p>
        <button type="button" onClick={() => this.setState({ failed: false })}>
          {t('error.retry')}
        </button>
      </div>
    );
  }
}

export const ErrorBoundary = withTranslation()(Boundary);
