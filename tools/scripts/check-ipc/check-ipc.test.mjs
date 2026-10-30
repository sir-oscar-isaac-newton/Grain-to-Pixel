import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { isStale } from './check-ipc.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const script = path.join(here, 'check-ipc.mjs');
const committed = path.resolve(here, '../../../packages/ipc-types/src/bindings.ts');

const run = (...args) => spawnSync(process.execPath, [script, ...args], { encoding: 'utf8' });

test('isStale ignores line endings but not content', () => {
  assert.equal(isStale('a\nb\n', 'a\r\nb\r\n'), false);
  assert.equal(isStale('a\nb\n', 'a\nc\n'), true);
});

test('the committed bindings are up to date (exit 0)', () => {
  const res = run();
  assert.equal(res.status, 0, res.stderr);
  assert.match(res.stdout, /check-ipc: OK/);
});

test('a stale bindings file fails (exit 1) and says how to fix it', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'gtp-ipc-test-'));
  try {
    const stale = path.join(dir, 'bindings.ts');
    writeFileSync(stale, readFileSync(committed, 'utf8') + '\n// stale edit\n');
    const res = run('--bindings', stale);
    assert.equal(res.status, 1);
    assert.match(res.stderr, /stale\. Run: pnpm gen:ipc/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a missing bindings file fails (exit 1)', () => {
  const res = run('--bindings', path.join(tmpdir(), 'gtp-does-not-exist.ts'));
  assert.equal(res.status, 1);
  assert.match(res.stderr, /missing/);
});
