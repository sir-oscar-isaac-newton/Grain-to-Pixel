import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { runSmoke } from './smoke.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const script = path.join(here, 'smoke.mjs');
const node = process.execPath;

// Fake "apps": node itself with an inline program.
const crashes = ['-e', 'process.exit(3)'];
const hangs = ['-e', 'setTimeout(() => {}, 60000)'];
const readyThenHangs = [
  '-e',
  "require('fs').writeFileSync(process.env.GTP_READY_FILE, 'x'); setTimeout(() => {}, 60000)",
];

test('an app that crashes at startup fails the smoke test', async () => {
  const res = await runSmoke({ binary: node, args: crashes, timeoutMs: 10_000 });
  assert.equal(res.ok, false);
  assert.match(res.reason, /exited before it was ready \(code 3/);
});

test('an app that never becomes ready fails after the timeout', async () => {
  const res = await runSmoke({ binary: node, args: hangs, timeoutMs: 1_500 });
  assert.equal(res.ok, false);
  assert.match(res.reason, /not ready within/);
});

test('an app that writes the ready file passes and is stopped afterwards', async () => {
  const res = await runSmoke({ binary: node, args: readyThenHangs, timeoutMs: 10_000 });
  assert.equal(res.ok, true);
});

test('a binary that does not exist fails', async () => {
  const res = await runSmoke({ binary: path.join(here, 'no-such-binary'), timeoutMs: 5_000 });
  assert.equal(res.ok, false);
});

test('the CLI exits 2 without --binary and 1 for a missing binary', () => {
  assert.equal(spawnSync(node, [script], { encoding: 'utf8' }).status, 2);
  const missing = spawnSync(node, [script, '--binary', path.join(here, 'nope'), '--timeout', '3'], {
    encoding: 'utf8',
  });
  assert.equal(missing.status, 1);
  assert.match(missing.stderr, /smoke: FAILED/);
});
