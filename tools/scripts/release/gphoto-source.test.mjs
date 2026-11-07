import assert from 'node:assert/strict';
import test from 'node:test';

import { FORMULAS, findProblems, parseBrewInfo, renderManifest } from './gphoto-source.mjs';

const formula = (name, over = {}) => ({
  name,
  license: 'LGPL-2.1-or-later',
  versions: { stable: '1.2.3' },
  installed: [{ version: '1.2.3' }],
  urls: { stable: { url: `https://example.org/${name}-1.2.3.tar.bz2`, checksum: 'a'.repeat(64) } },
  ...over,
});
const all = () => Object.keys(FORMULAS).map((name) => formula(name));

test('the entries are read from the Homebrew answer', () => {
  const [first] = parseBrewInfo([formula('libgphoto2')]);
  assert.deepEqual(first, {
    name: 'libgphoto2',
    provides: FORMULAS.libgphoto2,
    version: '1.2.3',
    installed: '1.2.3',
    licence: 'LGPL-2.1-or-later',
    url: 'https://example.org/libgphoto2-1.2.3.tar.bz2',
    sha256: 'a'.repeat(64),
  });
});

test('a complete set of installed formulae has no problems', () => {
  assert.deepEqual(findProblems(parseBrewInfo(all())), []);
});

test('a missing formula, a missing checksum or a different installed version is a problem', () => {
  assert.match(findProblems(parseBrewInfo(all().slice(1))).join(), /is not installed/);
  const noSum = all();
  noSum[0].urls.stable.checksum = null;
  assert.match(findProblems(parseBrewInfo(noSum)).join(), /no stable source address or checksum/);
  const other = all();
  other[1].installed = [{ version: '9.9' }];
  assert.match(
    findProblems(parseBrewInfo(other)).join(),
    /9\.9 is installed but the source is 1\.2\.3/,
  );
  // A Homebrew revision suffix is the same version.
  const revised = all();
  revised[2].installed = [{ version: '1.2.3_1' }];
  assert.deepEqual(findProblems(parseBrewInfo(revised)), []);
});

test('the manifest names every library with its licence, source and checksum', () => {
  const text = renderManifest(parseBrewInfo(all()));
  for (const name of Object.keys(FORMULAS)) assert.ok(text.includes(`${name} 1.2.3`), name);
  assert.match(text, /licence: LGPL-2\.1-or-later/);
  assert.match(text, /sha256: {2}a{64}/);
});
