#!/usr/bin/env node
// Fetches the source archives of the `gphoto2` and `libgphoto2_sys` crates (LGPL-2.1-only, compiled
// into the macOS app, H-09) for a release, the way rawler-source.mjs does for rawler: versions and
// checksums come from Cargo.lock, the archives from crates.io, and a download whose SHA-256 differs
// from the lock is refused. Prints the path of each archive, one per line.
// Usage: node tools/scripts/release/gphoto-crates-source.mjs <folder>
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { crateUrl, matchesChecksum, packageLock } from './rawler-source.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');

/** The crates compiled into the macOS app next to libgphoto2. */
export const CRATES = ['gphoto2', 'libgphoto2_sys'];

async function main(argv) {
  const folder = argv[0];
  if (!folder) {
    console.error('usage: gphoto-crates-source.mjs <folder>');
    return 2;
  }
  const lock = readFileSync(path.join(root, 'Cargo.lock'), 'utf8');
  mkdirSync(folder, { recursive: true });
  const files = [];
  for (const name of CRATES) {
    const entry = packageLock(lock, name);
    if (!entry) {
      console.error(`gphoto-crates-source: Cargo.lock has no ${name} with a checksum`);
      return 1;
    }
    const response = await fetch(crateUrl(name, entry.version));
    if (!response.ok) {
      console.error(`gphoto-crates-source: crates.io answered ${response.status} for ${name}`);
      return 1;
    }
    const bytes = Buffer.from(await response.arrayBuffer());
    if (!matchesChecksum(bytes, entry.checksum)) {
      console.error(
        `gphoto-crates-source: ${name}: the archive does not match the checksum in Cargo.lock`,
      );
      return 1;
    }
    const file = path.join(folder, `${name}-${entry.version}.crate`);
    writeFileSync(file, bytes);
    files.push(file);
  }
  for (const file of files) console.log(file);
  return 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main(process.argv.slice(2)).then(
    (code) => {
      process.exitCode = code;
    },
    (err) => {
      console.error(`gphoto-crates-source: ${err.message}`);
      process.exitCode = 2;
    },
  );
}
