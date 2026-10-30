import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const logClientEvent = vi.fn();
vi.mock('../ipc', () => ({
  logClientEvent: (level: string, message: string) => logClientEvent(level, message),
}));

import { ErrorBoundary } from './ErrorBoundary';
import { installGlobalErrorHandlers } from './globalHandlers';

const OK_TEXT = 'all good';

function Bomb({ explode }: { explode: boolean }) {
  if (explode) throw new Error('secret internal detail');
  return <p>{OK_TEXT}</p>;
}

describe('ErrorBoundary', () => {
  beforeEach(() => {
    logClientEvent.mockReset();
    // React reports caught render errors to the console; keep the test output clean.
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders its children when nothing fails', () => {
    render(
      <ErrorBoundary>
        <Bomb explode={false} />
      </ErrorBoundary>,
    );
    expect(screen.getByText('all good')).toBeInTheDocument();
    expect(logClientEvent).not.toHaveBeenCalled();
  });

  it('logs the error and shows a recoverable screen without technical text', async () => {
    const user = userEvent.setup();
    let explode = true;
    function Toggle() {
      return <Bomb explode={explode} />;
    }
    render(
      <ErrorBoundary>
        <Toggle />
      </ErrorBoundary>,
    );

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent('Something went wrong');
    expect(alert).not.toHaveTextContent('secret internal detail');
    expect(logClientEvent).toHaveBeenCalledWith(
      'Error',
      expect.stringContaining('secret internal detail'),
    );

    explode = false;
    await user.click(screen.getByRole('button', { name: 'Try again' }));
    expect(screen.getByText('all good')).toBeInTheDocument();
  });
});

describe('installGlobalErrorHandlers', () => {
  beforeEach(() => {
    logClientEvent.mockReset();
  });

  it('logs uncaught errors and unhandled rejections until uninstalled', () => {
    const target = new EventTarget() as unknown as Window;
    const uninstall = installGlobalErrorHandlers(target);

    target.dispatchEvent(Object.assign(new Event('error'), { message: 'boom' }));
    target.dispatchEvent(
      Object.assign(new Event('unhandledrejection'), { reason: new TypeError('nope') }),
    );
    expect(logClientEvent).toHaveBeenCalledWith('Error', 'window error: boom');
    expect(logClientEvent).toHaveBeenCalledWith('Error', 'unhandled rejection: TypeError: nope');

    uninstall();
    target.dispatchEvent(Object.assign(new Event('error'), { message: 'again' }));
    expect(logClientEvent).toHaveBeenCalledTimes(2);
  });
});
