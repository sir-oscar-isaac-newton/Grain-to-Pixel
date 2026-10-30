import { logClientEvent } from '../ipc';

function describe(reason: unknown): string {
  if (reason instanceof Error) return `${reason.name}: ${reason.message}`;
  return String(reason);
}

/** Reports uncaught errors and unhandled promise rejections to the log. Returns an uninstall function. */
export function installGlobalErrorHandlers(target: Window = window): () => void {
  const onError = (event: ErrorEvent) => {
    void logClientEvent('Error', `window error: ${event.message}`);
  };
  const onRejection = (event: PromiseRejectionEvent) => {
    void logClientEvent('Error', `unhandled rejection: ${describe(event.reason)}`);
  };
  target.addEventListener('error', onError);
  target.addEventListener('unhandledrejection', onRejection);
  return () => {
    target.removeEventListener('error', onError);
    target.removeEventListener('unhandledrejection', onRejection);
  };
}
