import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  findMissingNotices,
  groupEntries,
  isPermissive,
  isStale,
  jsEntries,
  readLicenceText,
  renderLicences,
  shippedRustPackages,
} from './check-notices.mjs';

test('an expression is permissive when its alternatives can all be permissive', () => {
  for (const ok of [
    'MIT',
    'Apache-2.0',
    'MIT OR Apache-2.0',
    'MIT/Apache-2.0',
    'Apache-2.0 WITH LLVM-exception',
    'Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT',
    '(MIT OR Apache-2.0) AND Unicode-3.0',
    'Unlicense OR MIT',
    'MIT OR LGPL-2.1',
  ]) {
    assert.equal(isPermissive(ok), true, ok);
  }
  for (const no of [
    'LGPL-2.1',
    'MPL-2.0',
    'GPL-3.0-only',
    '(MIT OR Apache-2.0) AND IJG',
    'MIT AND LGPL-2.1',
    'unknown',
    '',
    undefined,
  ]) {
    assert.equal(isPermissive(no), false, String(no));
  }
});

function metadata() {
  const pkg = (name, source = 'registry+x') => ({
    id: `${name} 1.0.0`,
    name,
    version: '1.0.0',
    source: source === null ? null : source,
    license: 'MIT',
    manifest_path: `/x/${name}/Cargo.toml`,
  });
  const dep = (name, kind = null) => ({
    pkg: `${name} 1.0.0`,
    dep_kinds: [{ kind, target: null }],
  });
  return {
    packages: ['app', 'lib', 'inner', 'macro', 'testonly', 'orphan'].map((n) =>
      pkg(n, n === 'app' ? null : 'registry+x'),
    ),
    resolve: {
      nodes: [
        { id: 'app 1.0.0', deps: [dep('lib'), dep('macro', 'build'), dep('testonly', 'dev')] },
        { id: 'lib 1.0.0', deps: [dep('inner')] },
        { id: 'inner 1.0.0', deps: [] },
      ],
    },
  };
}

test('only registry packages reached through normal dependencies ship', () => {
  const names = shippedRustPackages(metadata(), 'app')
    .map((p) => p.name)
    .sort();
  assert.deepEqual(names, ['inner', 'lib']);
  assert.throws(() => shippedRustPackages(metadata(), 'nothing'), /not in the workspace/);
});

test('licence files are read from the top of a package, and long ones are not licence texts', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'gtp-notices-'));
  try {
    writeFileSync(path.join(dir, 'LICENSE-MIT'), 'MIT text\r\nline  \r\n');
    writeFileSync(path.join(dir, 'LICENSE-APACHE'), 'Apache text');
    writeFileSync(path.join(dir, 'README.md'), 'not a licence');
    mkdirSync(path.join(dir, 'LICENSES'));
    writeFileSync(path.join(dir, 'COPYRIGHT'), 'x'.repeat(300_000));
    assert.equal(readLicenceText(dir), 'Apache text\n\nMIT text\nline');
    assert.equal(readLicenceText(path.join(dir, 'missing')), null);
    const empty = mkdtempSync(path.join(tmpdir(), 'gtp-notices-empty-'));
    assert.equal(readLicenceText(empty), null);
    rmSync(empty, { recursive: true });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

const entry = (name, licence, text) => ({
  ecosystem: 'crate',
  name,
  version: '1.0.0',
  licence,
  text,
});

test('packages with the same text are listed together, in a stable order', () => {
  const entries = [
    entry('zeta', 'MIT', 'same text'),
    entry('alpha', 'MIT', 'same text'),
    entry('mid', 'Apache-2.0', 'other text'),
    {
      ...entry('bare', 'MIT', null),
      authors: ['A. Author'],
      repository: 'https://example.org/bare',
    },
  ];
  const groups = groupEntries(entries);
  assert.equal(groups.length, 3);
  assert.deepEqual(
    groups.map((g) => g.entries.map((e) => e.name)),
    [['alpha', 'zeta'], ['bare'], ['mid']],
  );
  const text = renderLicences(entries);
  assert.match(text, /alpha 1\.0\.0 \(crate\)\n {2}zeta 1\.0\.0 \(crate\)/);
  assert.match(
    text,
    /This package ships no licence file\.\nAuthors: A\. Author\nSource: https:\/\/example\.org\/bare/,
  );
  assert.equal(
    renderLicences([...entries].reverse()),
    text,
    'the order of the input does not matter',
  );
});

test('the JavaScript packages of the workspace are not third party', () => {
  const entries = jsEntries({
    MIT: [
      { name: '@gtp/ipc-types', versions: ['0.0.0'], paths: ['/nowhere'], license: 'MIT' },
      { name: 'react', versions: ['18.3.1'], paths: ['/nowhere'], license: 'MIT', homepage: 'h' },
    ],
  });
  assert.deepEqual(
    entries.map((e) => `${e.name} ${e.version} ${e.ecosystem}`),
    ['react 18.3.1 npm'],
  );
});

test('a package with a non-permissive licence that the notices do not name is reported', () => {
  const entries = [
    entry('serde', 'MIT OR Apache-2.0', 'x'),
    entry('rawler', 'LGPL-2.1', 'x'),
    entry('selectors', 'MPL-2.0', 'x'),
    entry('jpeg-encoder', '(MIT OR Apache-2.0) AND IJG', 'x'),
  ];
  const incomplete = 'Credits: `rawler` and the rest.\n- `selectors-extra` is another crate';
  assert.deepEqual(findMissingNotices(entries, incomplete), [
    'jpeg-encoder ((MIT OR Apache-2.0) AND IJG)',
    'selectors (MPL-2.0)',
  ]);
  assert.deepEqual(
    findMissingNotices(entries, 'rawler, selectors and jpeg-encoder are named here'),
    [],
  );
});

test('line endings do not make the licences file stale', () => {
  assert.equal(isStale('a\r\nb\n', 'a\nb'), false);
  assert.equal(isStale('a\nb', 'a\nc'), true);
});
