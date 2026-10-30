import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// Tauri expects a fixed dev port (see src-tauri/tauri.conf.json devUrl).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: '127.0.0.1' },
  build: { target: 'es2022', outDir: 'dist', emptyOutDir: true },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test-setup.ts'],
    css: false,
  },
});
