#!/usr/bin/env node
// Fails when the committed IPC bindings are stale: regenerates them from the Rust sources into a
// temporary file and compares. Fix with `pnpm gen:ipc` and commit packages/ipc-types/src/bindings.ts.
// Usage: node tools/scripts/check-ipc/check-ipc.mjs [--bindings <path>]
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const DEFAULT_BINDINGS = path.join(root, 'packages/ipc-types/src/bindings.ts');

const normalize = (text) => text.replace(/\r\n/g, '\n');

/** True when the committed text differs from the freshly generated text (ignoring line endings). */
export function isStale(committed, generated) {
  return normalize(committed) !== normalize(generated);
}

function generate(outPath) {
  const res = spawnSync(
    process.env.CARGO || 'cargo',
    ['run', '-q', '-p', 'grain-to-pixel-desktop', '--example', 'gen_ipc', '--', outPath],
    { cwd: root, encoding: 'utf8' },
  );
  if (res.error || res.status !== 0) {
    throw new Error(`generating bindings failed: ${res.error?.message ?? res.stderr}`);
  }
}

function main(argv) {
  const i = argv.indexOf('--bindings');
  const committedPath = i >= 0 ? path.resolve(argv[i + 1]) : DEFAULT_BINDINGS;
  if (!existsSync(committedPath)) {
    console.error(`check-ipc: ${committedPath} is missing. Run: pnpm gen:ipc`);
    return 1;
  }
  const dir = mkdtempSync(path.join(tmpdir(), 'gtp-ipc-'));
  try {
    const generatedPath = path.join(dir, 'bindings.ts');
    generate(generatedPath);
    if (isStale(readFileSync(committedPath, 'utf8'), readFileSync(generatedPath, 'utf8'))) {
      console.error('check-ipc: IPC bindings are stale. Run: pnpm gen:ipc and commit the result.');
      return 1;
    }
    console.log('check-ipc: OK (bindings are up to date)');
    return 0;
  } catch (err) {
    console.error(`check-ipc: ${err.message}`);
    return 2;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  process.exitCode = main(process.argv.slice(2));
}
