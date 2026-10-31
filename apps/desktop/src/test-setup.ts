import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, vi } from 'vitest';
import './shared/i18n';

// jsdom has no object URLs; thumbnails use them.
let objectUrls = 0;
if (typeof URL.createObjectURL !== 'function') {
  URL.createObjectURL = vi.fn(() => `blob:test-${(objectUrls += 1)}`);
}
if (typeof URL.revokeObjectURL !== 'function') {
  URL.revokeObjectURL = vi.fn();
}

// jsdom has no ResizeObserver; Radix (the slider) needs one to exist, and there is nothing to
// observe without layout.
if (typeof ResizeObserver === 'undefined') {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}

afterEach(() => {
  cleanup();
});
