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

// jsdom has no PointerEvent, and without one the coordinates and button of a pointer event the
// tests send are lost. A mouse event with the pointer's id is all the code under test reads.
if (typeof PointerEvent === 'undefined') {
  globalThis.PointerEvent = class extends MouseEvent {
    readonly pointerId: number;
    constructor(type: string, init: PointerEventInit = {}) {
      super(type, init);
      this.pointerId = init.pointerId ?? 0;
    }
  } as unknown as typeof PointerEvent;
}

afterEach(() => {
  cleanup();
});
