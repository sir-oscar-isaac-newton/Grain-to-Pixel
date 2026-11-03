#!/usr/bin/env node
// Fetches the source archive of the pinned `rawler` (LGPL-2.1) for a release (Q-08, ADR 0007): the
// version and checksum come from Cargo.lock, the archive from crates.io, and a download whose
// SHA-256 differs from the lock is refused. Prints the path of the archive.
// Usage: node tools/scripts/release/rawler-source.mjs <folder>
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');

/** The version and checksum of the `rawler` package in a Cargo.lock, or null. */
export function rawlerLock(cargoLock) {
  for (const block of cargoLock.replace(/\r\n/g, '\n').split('\n[[package]]')) {
    if (!/^\s*name = "rawler"$/m.test(block)) continue;
    const version = /^version = "([^"]+)"$/m.exec(block)?.[1];
    const checksum = /^checksum = "([0-9a-f]{64})"$/m.exec(block)?.[1];
    return version && checksum ? { version, checksum } : null;
  }
  return null;
}

/** Where crates.io serves a crate archive from. */
export const crateUrl = (name, version) =>
  `https://static.crates.io/crates/${name}/${name}-${version}.crate`;

/** True when the bytes are the archive the lock names. */
export function matchesChecksum(bytes, checksum) {
  return createHash('sha256').update(bytes).digest('hex') === checksum;
}

async function main(argv) {
  const folder = argv[0];
  if (!folder) {
    console.error('usage: rawler-source.mjs <folder>');
    return 2;
  }
  const lock = rawlerLock(readFileSync(path.join(root, 'Cargo.lock'), 'utf8'));
  if (!lock) {
    console.error('rawler-source: Cargo.lock has no rawler with a checksum');
    return 1;
  }
  const response = await fetch(crateUrl('rawler', lock.version));
  if (!response.ok) {
    console.error(`rawler-source: crates.io answered ${response.status}`);
    return 1;
  }
  const bytes = Buffer.from(await response.arrayBuffer());
  if (!matchesChecksum(bytes, lock.checksum)) {
    console.error(
      'rawler-source: the downloaded archive does not match the checksum in Cargo.lock',
    );
    return 1;
  }
  mkdirSync(folder, { recursive: true });
  const file = path.join(folder, `rawler-${lock.version}.crate`);
  writeFileSync(file, bytes);
  console.log(file);
  return 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main(process.argv.slice(2)).then(
    (code) => {
      process.exitCode = code;
    },
    (err) => {
      console.error(`rawler-source: ${err.message}`);
      process.exitCode = 2;
    },
  );
}
