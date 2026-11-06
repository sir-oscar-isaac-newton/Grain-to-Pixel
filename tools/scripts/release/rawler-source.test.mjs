import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import test from 'node:test';

import { crateUrl, matchesChecksum, rawlerLock } from './rawler-source.mjs';

const sum = 'a'.repeat(64);
const lock = (extra = '') => `[[package]]
name = "rawexif"
version = "1.0.0"
checksum = "${'b'.repeat(64)}"

[[package]]
name = "rawler"
version = "0.8.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "${sum}"
${extra}
[[package]]
name = "serde"
version = "1.0.0"
`;

test('the rawler version and checksum are read from Cargo.lock', () => {
  assert.deepEqual(rawlerLock(lock()), { version: '0.8.0', checksum: sum });
  assert.deepEqual(rawlerLock(lock().replace(/\n/g, '\r\n')), { version: '0.8.0', checksum: sum });
});

test('a lock without rawler, or without its checksum, gives nothing', () => {
  assert.equal(rawlerLock('[[package]]\nname = "serde"\nversion = "1.0.0"\n'), null);
  assert.equal(rawlerLock('[[package]]\nname = "rawler"\nversion = "0.8.0"\nsource = "x"\n'), null);
});

test('the archive comes from crates.io and is checked against the lock', () => {
  assert.equal(
    crateUrl('rawler', '0.8.0'),
    'https://static.crates.io/crates/rawler/rawler-0.8.0.crate',
  );
  const bytes = Buffer.from('an archive');
  const digest = createHash('sha256').update(bytes).digest('hex');
  assert.equal(matchesChecksum(bytes, digest), true);
  assert.equal(matchesChecksum(Buffer.from('another'), digest), false);
});
