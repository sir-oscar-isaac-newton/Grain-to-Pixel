import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { findConfigProblems, sizeReport } from './check-bundle.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const script = path.join(here, 'check-bundle.mjs');

const good = () => ({
  bundle: {
    active: true,
    targets: ['nsis', 'dmg'],
    macOS: { signingIdentity: '-' },
    windows: { nsis: { installMode: 'currentUser' } },
  },
});

function run(args) {
  return spawnSync(process.execPath, [script, ...args], { encoding: 'utf8' });
}

test('the real tauri.conf.json has the installer settings', () => {
  const result = run(['config']);
  assert.equal(result.status, 0, result.stderr);
});

test('valid settings have no problems', () => {
  assert.deepEqual(findConfigProblems(good()), []);
});

test('each drifted setting is reported', () => {
  const inactive = good();
  inactive.bundle.active = false;
  assert.match(findConfigProblems(inactive).join(), /active/);

  const noDmg = good();
  noDmg.bundle.targets = ['nsis'];
  assert.match(findConfigProblems(noDmg).join(), /"dmg"/);

  const noNsis = good();
  noNsis.bundle.targets = ['dmg'];
  assert.match(findConfigProblems(noNsis).join(), /"nsis"/);

  const unsigned = good();
  unsigned.bundle.macOS.signingIdentity = 'Developer ID';
  assert.match(findConfigProblems(unsigned).join(), /signingIdentity/);

  const perMachine = good();
  perMachine.bundle.windows.nsis.installMode = 'perMachine';
  assert.match(findConfigProblems(perMachine).join(), /installMode/);

  assert.deepEqual(findConfigProblems({}), ['missing the "bundle" section']);
});

test('the size is in megabytes and the limit is inclusive', () => {
  const mb = 1024 * 1024;
  assert.equal(sizeReport(40 * mb).ok, true);
  assert.equal(sizeReport(40 * mb + 1).ok, false);
  assert.equal(sizeReport(10 * mb).line, '10.0 MB (limit 40 MB)');
});

test('size fails over the limit only when enforced', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'check-bundle-'));
  try {
    const file = path.join(dir, 'big.exe');
    writeFileSync(file, Buffer.alloc(2 * 1024 * 1024));
    assert.equal(run(['size', file, '--max-mb', '1']).status, 0);
    assert.equal(run(['size', file, '--max-mb', '1', '--enforce']).status, 1);
    assert.equal(run(['size', file, '--max-mb', '3', '--enforce']).status, 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
