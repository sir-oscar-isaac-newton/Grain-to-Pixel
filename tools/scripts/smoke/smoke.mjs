#!/usr/bin/env node
// Smoke test: start the built app and require it to reach "ready" (page loaded) without crashing.
// The app writes the file named by GTP_READY_FILE once its page has loaded (see src-tauri/src/lib.rs).
// Usage: node tools/scripts/smoke/smoke.mjs --binary <path> [--timeout <seconds>]
// Exit codes: 0 ready, 1 crashed or not ready in time, 2 usage error.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function stopTree(child) {
  if (child.exitCode !== null || child.pid === undefined) return;
  if (process.platform === 'win32') {
    spawnSync('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
  } else {
    child.kill('SIGKILL');
  }
}

/**
 * @param {{binary: string, args?: string[], timeoutMs?: number}} options
 * @returns {Promise<{ok: boolean, reason: string}>}
 */
export async function runSmoke({ binary, args = [], timeoutMs = 90_000 }) {
  const dir = mkdtempSync(path.join(tmpdir(), 'gtp-smoke-'));
  const readyFile = path.join(dir, 'ready');
  let exit = null;
  const child = spawn(binary, args, {
    env: { ...process.env, GTP_READY_FILE: readyFile },
    stdio: 'ignore',
  });
  child.on('exit', (code, signal) => {
    exit = { code, signal };
  });
  child.on('error', (err) => {
    exit = { code: null, signal: null, error: err.message };
  });

  try {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (existsSync(readyFile)) return { ok: true, reason: 'app is ready' };
      if (exit) {
        const why = exit.error ?? `code ${exit.code ?? 'none'}, signal ${exit.signal ?? 'none'}`;
        return { ok: false, reason: `app exited before it was ready (${why})` };
      }
      await sleep(250);
    }
    return { ok: false, reason: `app was not ready within ${timeoutMs / 1000} s` };
  } finally {
    stopTree(child);
    rmSync(dir, { recursive: true, force: true });
  }
}

async function main(argv) {
  const get = (flag) => {
    const i = argv.indexOf(flag);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const binary = get('--binary');
  if (!binary) {
    console.error('usage: smoke.mjs --binary <path> [--timeout <seconds>]');
    return 2;
  }
  const timeoutMs = Number(get('--timeout') ?? 90) * 1000;
  const result = await runSmoke({ binary: path.resolve(binary), timeoutMs });
  if (result.ok) {
    console.log(`smoke: OK (${result.reason})`);
    return 0;
  }
  console.error(`smoke: FAILED (${result.reason})`);
  return 1;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  process.exitCode = await main(process.argv.slice(2));
}
