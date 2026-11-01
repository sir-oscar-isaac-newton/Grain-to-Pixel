import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import {
  findConfigProblems,
  findReleaseProblems,
  PUBKEY_PLACEHOLDER,
  sizeReport,
  workspaceVersion,
} from './check-bundle.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const script = path.join(here, 'check-bundle.mjs');

const good = () => ({
  version: '0.1.0',
  bundle: {
    active: true,
    targets: ['nsis', 'dmg'],
    macOS: { signingIdentity: '-' },
    windows: { nsis: { installMode: 'currentUser' } },
  },
  plugins: {
    updater: {
      endpoints: ['https://github.com/o/r/releases/latest/download/latest.json'],
      pubkey: 'dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk=',
      requireSignedVersion: true,
    },
  },
});

const cargo = (version = '0.1.0') =>
  `[workspace]\nmembers = ["a"]\n\n[workspace.package]\nedition = "2021"\nversion = "${version}"\n\n[workspace.lints.rust]\nunsafe_code = "forbid"\n`;

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

test('each drifted updater setting is reported', () => {
  const noUpdater = good();
  delete noUpdater.plugins;
  assert.match(findConfigProblems(noUpdater).join(), /plugins\.updater/);

  const plain = good();
  plain.plugins.updater.endpoints = ['http://example.com/latest.json'];
  assert.match(findConfigProblems(plain).join(), /https/);

  const noKey = good();
  noKey.plugins.updater.pubkey = ' ';
  assert.match(findConfigProblems(noKey).join(), /pubkey/);

  const downgrade = good();
  downgrade.plugins.updater.requireSignedVersion = false;
  assert.match(findConfigProblems(downgrade).join(), /requireSignedVersion/);
});

test('the version is read from the workspace package table only', () => {
  assert.equal(workspaceVersion(cargo('1.2.3')), '1.2.3');
  assert.equal(workspaceVersion('[package]\nversion = "9.9.9"\n'), null);
  assert.equal(workspaceVersion('[workspace.package]\nversion = "0.0.1-rc.1"'), '0.0.1-rc.1');
});

test('a release needs a tag that matches both versions and a real updater key', () => {
  assert.deepEqual(findReleaseProblems(good(), cargo(), 'v0.1.0'), []);

  const rc = good();
  rc.version = '0.1.0-rc.1';
  assert.deepEqual(findReleaseProblems(rc, cargo('0.1.0-rc.1'), 'v0.1.0-rc.1'), []);

  assert.match(findReleaseProblems(good(), cargo(), '0.1.0').join(), /not vX\.Y\.Z/);
  assert.match(findReleaseProblems(good(), cargo(), 'v0.1').join(), /not vX\.Y\.Z/);
  assert.match(findReleaseProblems(good(), cargo(), 'v0.2.0').join(), /tauri\.conf\.json version/);
  assert.match(findReleaseProblems(good(), cargo('0.0.9'), 'v0.1.0').join(), /Cargo\.toml/);

  const placeholder = good();
  placeholder.plugins.updater.pubkey = PUBKEY_PLACEHOLDER;
  assert.match(findReleaseProblems(placeholder, cargo(), 'v0.1.0').join(), /public key/);
});

test('release mode reads the real files and rejects a tag of another version', () => {
  const result = run(['release', 'v9.9.9']);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /tauri\.conf\.json version/);
  assert.equal(run(['release', 'nonsense']).status, 1);
  assert.equal(run(['release']).status, 2);
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
