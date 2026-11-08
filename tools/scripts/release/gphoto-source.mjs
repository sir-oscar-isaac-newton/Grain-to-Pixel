#!/usr/bin/env node
// Fetches the source archives of the libraries the macOS app bundles next to libgphoto2 (LGPL-2.1,
// ADR 0004 amendment 9, H-09), so a release can offer them the way it offers rawler's source
// (rawler-source.mjs). Run on the macOS runner that built the app, after
// `brew install libgphoto2 pkgconf`: Homebrew says which versions are installed, where their
// source is and its SHA-256, and an archive whose checksum differs is refused. Writes the archives
// and `gphoto-libraries.txt` (name, version, licence, source address) into <folder> and prints the
// path of each file, one per line.
// Usage: node tools/scripts/release/gphoto-source.mjs <folder>
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

import { matchesChecksum } from './rawler-source.mjs';

/**
 * The Homebrew formulae behind the seven bundled libraries (tools/scripts/release/bundle-libgphoto2.sh
 * prints the libraries it finds; a new one means a new formula here).
 */
export const FORMULAS = {
  libgphoto2: 'libgphoto2, libgphoto2_port and the drivers ptp2 and usb1',
  libtool: 'libltdl',
  libexif: 'libexif',
  libusb: 'libusb',
  'jpeg-turbo': 'libjpeg',
  gettext: 'libintl',
};

/** The bundled files of a Homebrew `info --json=v1` answer: one entry per formula. */
export function parseBrewInfo(json) {
  return json.map((formula) => ({
    name: formula.name,
    provides: FORMULAS[formula.name] ?? '?',
    version: formula.versions?.stable ?? null,
    installed: formula.installed?.[0]?.version ?? null,
    licence: formula.license ?? 'unknown',
    url: formula.urls?.stable?.url ?? null,
    sha256: formula.urls?.stable?.checksum ?? null,
  }));
}

/** Every reason the entries cannot be offered as they are; empty when they can. */
export function findProblems(entries) {
  const problems = [];
  for (const name of Object.keys(FORMULAS)) {
    if (!entries.some((entry) => entry.name === name)) problems.push(`${name} is not installed`);
  }
  for (const entry of entries) {
    if (!entry.version || !entry.url || !entry.sha256) {
      problems.push(`${entry.name}: Homebrew gives no stable source address or checksum`);
    } else if (entry.installed !== null && !entry.installed.startsWith(entry.version)) {
      // The bundled build must be the version whose source is offered.
      problems.push(
        `${entry.name}: ${entry.installed} is installed but the source is ${entry.version}`,
      );
    }
  }
  return problems;
}

/** The text of gphoto-libraries.txt. */
export function renderManifest(entries) {
  const lines = [
    'Libraries bundled in the macOS app of Grain to Pixel next to libgphoto2 (dynamic libraries,',
    'Contents/Frameworks/gphoto). The source archives of these exact versions are attached to the',
    'release next to this file. Licences are as Homebrew lists them for the formula.',
    '',
  ];
  for (const entry of entries) {
    lines.push(
      `${entry.name} ${entry.version} (${entry.provides})`,
      `  licence: ${entry.licence}`,
      `  source:  ${entry.url}`,
      `  sha256:  ${entry.sha256}`,
      '',
    );
  }
  return lines.join('\n');
}

async function main(argv) {
  const folder = argv[0];
  if (!folder) {
    console.error('usage: gphoto-source.mjs <folder>');
    return 2;
  }
  const entries = parseBrewInfo(
    JSON.parse(
      execFileSync('brew', ['info', '--json=v1', ...Object.keys(FORMULAS)], { encoding: 'utf8' }),
    ),
  );
  const problems = findProblems(entries);
  if (problems.length) {
    for (const problem of problems) console.error(`gphoto-source: ${problem}`);
    return 1;
  }
  mkdirSync(folder, { recursive: true });
  const files = [];
  for (const entry of entries) {
    const response = await fetch(entry.url);
    if (!response.ok) {
      console.error(`gphoto-source: ${entry.url} answered ${response.status}`);
      return 1;
    }
    const bytes = Buffer.from(await response.arrayBuffer());
    if (!matchesChecksum(bytes, entry.sha256)) {
      console.error(`gphoto-source: ${entry.name}: the archive does not match Homebrew's checksum`);
      return 1;
    }
    const file = path.join(folder, path.basename(new URL(entry.url).pathname));
    writeFileSync(file, bytes);
    files.push(file);
  }
  const manifest = path.join(folder, 'gphoto-libraries.txt');
  writeFileSync(manifest, renderManifest(entries));
  for (const file of [manifest, ...files]) console.log(file);
  return 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main(process.argv.slice(2)).then(
    (code) => {
      process.exitCode = code;
    },
    (err) => {
      console.error(`gphoto-source: ${err.message}`);
      process.exitCode = 2;
    },
  );
}
