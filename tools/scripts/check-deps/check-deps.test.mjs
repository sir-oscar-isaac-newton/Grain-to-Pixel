import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { ALLOWED, findViolations } from './check-deps.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const script = path.join(here, 'check-deps.mjs');
const repoManifest = path.resolve(here, '../../../Cargo.toml');
const badManifest = path.join(here, 'tests/fixtures/bad-workspace/Cargo.toml');

const run = (manifest) =>
  spawnSync(process.execPath, [script, '--manifest-path', manifest], { encoding: 'utf8' });

test('allowed edges pass', () => {
  const pkgs = [
    { name: 'gtp-core', dependencies: [] },
    { name: 'gtp-io', dependencies: [{ name: 'gtp-core' }, { name: 'serde' }] },
    { name: 'gtp-app', dependencies: [{ name: 'gtp-edit' }, { name: 'gtp-negative' }] },
    { name: 'gtp-edit', dependencies: [{ name: 'gtp-core' }] },
    { name: 'gtp-negative', dependencies: [{ name: 'gtp-core' }] },
  ];
  assert.deepEqual(findViolations(pkgs, ALLOWED), []);
});

test('feature crates may not depend on each other', () => {
  const pkgs = [
    { name: 'gtp-edit', dependencies: [{ name: 'gtp-negative' }] },
    { name: 'gtp-negative', dependencies: [] },
  ];
  const v = findViolations(pkgs, ALLOWED);
  assert.equal(v.length, 1);
  assert.match(v[0], /gtp-edit -> gtp-negative is not allowed/);
});

test('core may not depend on any workspace crate', () => {
  const pkgs = [
    { name: 'gtp-core', dependencies: [{ name: 'gtp-io' }] },
    { name: 'gtp-io', dependencies: [] },
  ];
  assert.equal(findViolations(pkgs, ALLOWED).length, 1);
});

test('unknown workspace packages are reported', () => {
  const v = findViolations([{ name: 'gtp-mystery', dependencies: [] }], ALLOWED);
  assert.match(v[0], /unknown workspace package "gtp-mystery"/);
});

test('this workspace passes (exit 0)', () => {
  const res = run(repoManifest);
  assert.equal(res.status, 0, res.stderr);
  assert.match(res.stdout, /check-deps: OK/);
});

test('bad fixture workspace fails (exit 1) and names the edge', () => {
  const res = run(badManifest);
  assert.equal(res.status, 1);
  assert.match(res.stderr, /FORBIDDEN: gtp-edit -> gtp-negative is not allowed/);
});
