#!/usr/bin/env node
// Third-party notices (Q-08, ADR 0007).
//
//   node tools/scripts/check-notices/check-notices.mjs            checks
//   node tools/scripts/check-notices/check-notices.mjs generate   writes docs/third-party-licenses.txt
//
// The licence text of every package that ships in the app (the Rust crates the desktop app links,
// for the two platforms we ship, and the production JavaScript packages) is collected from the
// package sources into docs/third-party-licenses.txt, with identical texts listed once. The check
// fails when that file is stale, and when a package whose licence is not permissive (LGPL, IJG,
// MPL ...) is not named in docs/third-party-notices.md, which carries the credits and the
// obligations that need words of their own.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const LICENCES_FILE = path.join(root, 'docs/third-party-licenses.txt');
const NOTICES_FILE = path.join(root, 'docs/third-party-notices.md');

/** The platforms we ship (specification section 1.1). */
export const TARGETS = ['x86_64-pc-windows-msvc', 'aarch64-apple-darwin'];
/** The crate whose dependency tree is what the app links. */
export const ROOT_CRATE = 'grain-to-pixel-desktop';

/** SPDX identifiers that ask for nothing beyond keeping the licence text with the binary. */
export const PERMISSIVE = new Set([
  'MIT',
  'Apache-2.0',
  'BSD-2-Clause',
  'BSD-3-Clause',
  'ISC',
  'Unicode-3.0',
  'Zlib',
  'CC0-1.0',
  'Unlicense',
  '0BSD',
  'BSL-1.0',
]);

const LICENCE_FILE = /^(licen[cs]e|copying|copyright|unlicen[cs]e|notice)([-_.].*)?$/i;
/** Files longer than this are not licence texts (a changelog named COPYRIGHT.md, say). */
const MAX_LICENCE_BYTES = 200_000;

const normalize = (text) =>
  text
    .replace(/\r\n/g, '\n')
    .replace(/[ \t]+$/gm, '')
    .trim();

// ---------- SPDX expressions ----------

function tokens(expression) {
  return expression
    .replace(/\//g, ' OR ')
    .replace(/([()])/g, ' $1 ')
    .split(/\s+/)
    .filter(Boolean);
}

/** True when the expression can be satisfied by permissive licences alone (OR picks, AND needs all). */
export function isPermissive(expression) {
  if (!expression) return false;
  const list = tokens(expression);
  let at = 0;
  const term = () => {
    const t = list[at++];
    if (t === '(') {
      const value = or();
      at++; // the closing parenthesis
      return value;
    }
    if (list[at] === 'WITH') at += 2; // an exception clause does not change the licence
    return PERMISSIVE.has(t?.replace(/\+$/, ''));
  };
  const and = () => {
    let value = term();
    while (list[at] === 'AND') {
      at++;
      value = term() && value;
    }
    return value;
  };
  const or = () => {
    let value = and();
    while (list[at] === 'OR') {
      at++;
      value = and() || value;
    }
    return value;
  };
  return or();
}

// ---------- what ships ----------

/**
 * The registry packages the app links, from the output of `cargo metadata` for one target: those
 * reachable from `rootName` through normal dependencies (build and dev dependencies do not ship).
 * Workspace crates are ours and are left out.
 */
export function shippedRustPackages(metadata, rootName = ROOT_CRATE) {
  const byId = new Map(metadata.packages.map((p) => [p.id, p]));
  const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
  const start = metadata.packages.find((p) => p.name === rootName && !p.source);
  if (!start) throw new Error(`the crate ${rootName} is not in the workspace`);
  const seen = new Set([start.id]);
  const queue = [start.id];
  while (queue.length) {
    const node = nodes.get(queue.pop());
    for (const dep of node?.deps ?? []) {
      if (!dep.dep_kinds.some((kind) => kind.kind === null) || seen.has(dep.pkg)) continue;
      seen.add(dep.pkg);
      queue.push(dep.pkg);
    }
  }
  return [...seen].map((id) => byId.get(id)).filter((p) => p?.source);
}

/** The text of the licence files at the top of a package directory, or null when it has none. */
export function readLicenceText(dir, licenceFile) {
  if (!dir || !existsSync(dir)) return null;
  const names = new Set(readdirSync(dir).filter((name) => LICENCE_FILE.test(name)));
  if (licenceFile) names.add(path.basename(licenceFile));
  const texts = [];
  for (const name of [...names].sort()) {
    const file = path.join(dir, name);
    if (!existsSync(file) || !statSync(file).isFile() || statSync(file).size > MAX_LICENCE_BYTES) {
      continue;
    }
    texts.push(normalize(readFileSync(file, 'utf8')));
  }
  const text = texts.filter(Boolean).join('\n\n');
  return text || null;
}

/** One package that ships: where it is from, its licence expression and its licence text. */
function entryOf(ecosystem, name, version, licence, text, extra) {
  return { ecosystem, name, version, licence: licence || 'unknown', text, ...extra };
}

export function rustEntries(packagesByTarget) {
  const unique = new Map();
  for (const packages of packagesByTarget) {
    for (const pkg of packages) unique.set(`${pkg.name}@${pkg.version}`, pkg);
  }
  return [...unique.values()].map((pkg) =>
    entryOf(
      'crate',
      pkg.name,
      pkg.version,
      pkg.license,
      readLicenceText(path.dirname(pkg.manifest_path), pkg.license_file),
      { repository: pkg.repository, authors: pkg.authors },
    ),
  );
}

/** From `pnpm licenses list --prod --json -r`: licence name to the packages that have it. */
export function jsEntries(licences) {
  const entries = [];
  for (const [licence, packages] of Object.entries(licences)) {
    for (const pkg of packages) {
      if (pkg.name.startsWith('@gtp/')) continue;
      pkg.versions.forEach((version, i) => {
        entries.push(
          entryOf('npm', pkg.name, version, pkg.license ?? licence, readLicenceText(pkg.paths[i]), {
            repository: pkg.homepage,
            authors: pkg.author ? [pkg.author] : [],
          }),
        );
      });
    }
  }
  return entries;
}

// ---------- the licences file ----------

/** The text a package without a licence file gets: what is known, so a reader can find the rest. */
function fallbackText(entry) {
  const lines = [`Licence: ${entry.licence}. This package ships no licence file.`];
  if (entry.authors?.length) lines.push(`Authors: ${entry.authors.join(', ')}`);
  if (entry.repository) lines.push(`Source: ${entry.repository}`);
  return lines.join('\n');
}

/** Packages grouped by identical licence text, in a stable order. */
export function groupEntries(entries) {
  const groups = new Map();
  for (const entry of entries) {
    const text = entry.text ?? fallbackText(entry);
    const key = createHash('sha256').update(text).digest('hex');
    const group = groups.get(key) ?? { text, entries: [] };
    group.entries.push(entry);
    groups.set(key, group);
  }
  const list = [...groups.values()];
  for (const group of list) {
    group.entries.sort((a, b) => `${a.name} ${a.version}`.localeCompare(`${b.name} ${b.version}`));
  }
  return list.sort((a, b) =>
    `${a.entries[0].name} ${a.entries[0].version}`.localeCompare(
      `${b.entries[0].name} ${b.entries[0].version}`,
    ),
  );
}

const RULE = '='.repeat(78);

export function renderLicences(entries) {
  const groups = groupEntries(entries);
  const out = [
    'Third-party licences',
    '',
    'The licence texts of the packages that ship in Grain to Pixel: the Rust crates the app links and',
    'the JavaScript packages in its window. Packages with the same text are listed together. The',
    'credits and obligations that need words of their own are in docs/third-party-notices.md.',
    '',
    'Generated by tools/scripts/check-notices (pnpm gen:notices). Do not edit by hand.',
    '',
  ];
  for (const group of groups) {
    const licences = [...new Set(group.entries.map((e) => e.licence))].sort().join('; ');
    out.push(RULE, `Licence: ${licences}`, 'Packages:');
    for (const e of group.entries) out.push(`  ${e.name} ${e.version} (${e.ecosystem})`);
    out.push('', group.text, '');
  }
  return `${out.join('\n')}\n`;
}

// ---------- the check ----------

/** The packages whose licence is not permissive and that the notices file does not name. */
export function findMissingNotices(entries, notices) {
  const missing = new Set();
  for (const entry of entries) {
    if (isPermissive(entry.licence)) continue;
    const named = new RegExp(
      `(^|[^\\w-])${entry.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}([^\\w-]|$)`,
    );
    if (!named.test(notices)) missing.add(`${entry.name} (${entry.licence})`);
  }
  return [...missing].sort();
}

export function isStale(committed, generated) {
  return normalize(committed) !== normalize(generated);
}

// ---------- running ----------

function run(command, args) {
  const res = spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 512 * 1024 * 1024,
    shell: process.platform === 'win32' && command === 'pnpm',
  });
  if (res.error || res.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} failed: ${res.error?.message ?? res.stderr}`);
  }
  return res.stdout;
}

function collect() {
  const rust = TARGETS.map((target) =>
    shippedRustPackages(
      JSON.parse(
        run(process.env.CARGO || 'cargo', [
          'metadata',
          '--format-version',
          '1',
          '--filter-platform',
          target,
          // What the macOS installer ships: the libgphoto2 binding is behind this feature (and only
          // on macOS, so the Windows list is the same with or without it).
          '--features',
          `${ROOT_CRATE}/tether`,
        ]),
      ),
    ),
  );
  const js = JSON.parse(run('pnpm', ['licenses', 'list', '--prod', '--json', '-r']));
  return [...rustEntries(rust), ...jsEntries(js)];
}

function main(argv) {
  const entries = collect();
  const text = renderLicences(entries);
  if (argv[0] === 'generate') {
    writeFileSync(LICENCES_FILE, text);
    console.log(
      `check-notices: wrote ${path.relative(root, LICENCES_FILE)} (${entries.length} packages)`,
    );
    return 0;
  }
  const problems = [];
  const missing = findMissingNotices(entries, readFileSync(NOTICES_FILE, 'utf8'));
  for (const name of missing) {
    problems.push(`${name} needs a notice in docs/third-party-notices.md`);
  }
  if (!existsSync(LICENCES_FILE) || isStale(readFileSync(LICENCES_FILE, 'utf8'), text)) {
    problems.push('docs/third-party-licenses.txt is stale. Run: pnpm gen:notices and commit it');
  }
  for (const problem of problems) console.error(`check-notices: ${problem}`);
  if (!problems.length) console.log(`check-notices: OK (${entries.length} packages)`);
  return problems.length ? 1 : 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (err) {
    console.error(`check-notices: ${err.message}`);
    process.exitCode = 2;
  }
}
