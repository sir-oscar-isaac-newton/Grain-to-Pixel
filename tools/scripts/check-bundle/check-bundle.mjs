#!/usr/bin/env node
// Checks the installer settings, the installer size and a release tag (docs/install.md,
// docs/release.md, specification section 15).
// Usage: node tools/scripts/check-bundle/check-bundle.mjs config [--file <tauri.conf.json>]
//        node tools/scripts/check-bundle/check-bundle.mjs size <installer> [--max-mb <n>] [--enforce]
//        node tools/scripts/check-bundle/check-bundle.mjs release <tag>
// `config` fails when the bundle or updater settings drift, or when a file a shipped build is made\n// from turns on the e2e debugging port (docs/convention.md section 8); `size` prints the size and fails over
// the limit only with --enforce (the Windows installer), otherwise it only reports; `release`
// fails unless the tag, the app version and the updater key are ready for a release.
import { readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const DEFAULT_CONF = path.join(root, 'apps/desktop/src-tauri/tauri.conf.json');
const CARGO_TOML = path.join(root, 'Cargo.toml');
/** What tauri.conf.json holds for the updater public key until the real key is put there. */
export const PUBKEY_PLACEHOLDER = 'REPLACE_WITH_UPDATER_PUBLIC_KEY';
/** What the e2e build adds (src-tauri/tauri.e2e.conf.json); no shipped build may switch it on. */
const E2E_CONF_NAME = 'tauri.e2e.conf';
const DEBUG_PORT_FLAG = /remote-debugging-port/;
/** The files a shipped build is made from, relative to the repository root. */
const SHIPPED_FILES = [
  'apps/desktop/src-tauri/tauri.conf.json',
  'apps/desktop/src-tauri/tauri.release.conf.json',
  '.github/workflows/release.yml',
  '.github/workflows/bundle.yml',
];
const TAG = /^v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/;
export const MAX_INSTALLER_MB = 40;

/** Every problem found in the parsed tauri.conf.json; empty when the installer settings are right. */
export function findConfigProblems(conf) {
  const bundle = conf?.bundle;
  if (!bundle) return ['missing the "bundle" section'];
  const problems = [];
  if (bundle.active !== true) problems.push('bundle.active must be true');
  const targets = Array.isArray(bundle.targets) ? bundle.targets : [];
  for (const target of ['nsis', 'dmg']) {
    if (!targets.includes(target)) problems.push(`bundle.targets must include "${target}"`);
  }
  if (bundle.macOS?.signingIdentity !== '-') {
    problems.push('bundle.macOS.signingIdentity must be "-" (ad-hoc signing)');
  }
  if (bundle.windows?.nsis?.installMode !== 'currentUser') {
    problems.push('bundle.windows.nsis.installMode must be "currentUser" (no admin rights)');
  }
  const updater = conf.plugins?.updater;
  if (!updater) return [...problems, 'missing the "plugins.updater" section'];
  const endpoints = Array.isArray(updater.endpoints) ? updater.endpoints : [];
  if (endpoints.length === 0 || !endpoints.every((url) => String(url).startsWith('https://'))) {
    problems.push('plugins.updater.endpoints must be https URLs');
  }
  if (typeof updater.pubkey !== 'string' || updater.pubkey.trim() === '') {
    problems.push('plugins.updater.pubkey must be set');
  }
  if (updater.requireSignedVersion !== true) {
    problems.push('plugins.updater.requireSignedVersion must be true (no forced downgrades)');
  }
  return problems;
}

/** Every shipped file (name to text) that turns on the e2e debugging port; empty when none does. */
export function findDebugPortProblems(files) {
  return Object.entries(files)
    .filter(([, text]) => text.includes(E2E_CONF_NAME) || DEBUG_PORT_FLAG.test(text))
    .map(
      ([name]) =>
        `${name} must not turn on the e2e debugging port (tauri.e2e.conf.json is for the e2e job only)`,
    );
}

/** The version in the `[workspace.package]` table of a Cargo.toml, or null. */
export function workspaceVersion(cargoToml) {
  const table = /\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/.exec(cargoToml);
  return table ? (/^\s*version\s*=\s*"([^"]+)"/m.exec(table[1])?.[1] ?? null) : null;
}

/** Every reason the tag cannot be released yet; empty when it can. */
export function findReleaseProblems(conf, cargoToml, tag) {
  const match = TAG.exec(tag);
  if (!match) return [`the tag "${tag}" is not vX.Y.Z or vX.Y.Z-suffix`];
  const version = match[1];
  const problems = [];
  if (conf?.version !== version) {
    problems.push(`tauri.conf.json version is "${conf?.version}", the tag says "${version}"`);
  }
  const cargo = workspaceVersion(cargoToml);
  if (cargo !== version) {
    problems.push(
      `Cargo.toml [workspace.package] version is "${cargo}", the tag says "${version}"`,
    );
  }
  const pubkey = conf?.plugins?.updater?.pubkey;
  if (typeof pubkey !== 'string' || pubkey.trim() === '' || pubkey.trim() === PUBKEY_PLACEHOLDER) {
    problems.push('the updater public key in tauri.conf.json is not set');
  }
  return problems;
}

/** The size line for an installer, and whether it is within the limit. */
export function sizeReport(bytes, maxMb = MAX_INSTALLER_MB) {
  const mb = bytes / (1024 * 1024);
  return { mb, ok: mb <= maxMb, line: `${mb.toFixed(1)} MB (limit ${maxMb} MB)` };
}

function option(args, name) {
  const at = args.indexOf(name);
  return at >= 0 ? args[at + 1] : undefined;
}

function main(args) {
  const [mode, ...rest] = args;
  if (mode === 'config') {
    const file = option(rest, '--file') ?? DEFAULT_CONF;
    const problems = findConfigProblems(JSON.parse(readFileSync(file, 'utf8')));
    if (file === DEFAULT_CONF) {
      const shipped = Object.fromEntries(
        SHIPPED_FILES.map((name) => [name, readFileSync(path.join(root, name), 'utf8')]),
      );
      problems.push(...findDebugPortProblems(shipped));
    }
    for (const problem of problems) console.error(`check-bundle: ${problem}`);
    return problems.length ? 1 : 0;
  }
  if (mode === 'release' && rest[0]) {
    const conf = JSON.parse(readFileSync(DEFAULT_CONF, 'utf8'));
    const problems = findReleaseProblems(conf, readFileSync(CARGO_TOML, 'utf8'), rest[0]);
    for (const problem of problems) console.error(`check-bundle: ${problem}`);
    return problems.length ? 1 : 0;
  }
  if (mode === 'size' && rest[0]) {
    const report = sizeReport(
      statSync(rest[0]).size,
      Number(option(rest, '--max-mb') ?? MAX_INSTALLER_MB),
    );
    console.log(`${path.basename(rest[0])}: ${report.line}`);
    if (!report.ok && rest.includes('--enforce')) {
      console.error('check-bundle: the installer is over the size limit');
      return 1;
    }
    return 0;
  }
  console.error(
    'usage: check-bundle.mjs config [--file f] | size <installer> [--max-mb n] [--enforce] | release <tag>',
  );
  return 2;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  process.exitCode = main(process.argv.slice(2));
}
