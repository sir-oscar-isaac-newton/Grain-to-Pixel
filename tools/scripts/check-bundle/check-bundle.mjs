#!/usr/bin/env node
// Checks the installer settings and the installer size (docs/install.md, specification section 15).
// Usage: node tools/scripts/check-bundle/check-bundle.mjs config [--file <tauri.conf.json>]
//        node tools/scripts/check-bundle/check-bundle.mjs size <installer> [--max-mb <n>] [--enforce]
// `config` fails when the bundle settings drift; `size` prints the size and fails over the limit
// only with --enforce (the Windows installer), otherwise it only reports.
import { readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const DEFAULT_CONF = path.join(root, 'apps/desktop/src-tauri/tauri.conf.json');
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
    'usage: check-bundle.mjs config [--file f] | size <installer> [--max-mb n] [--enforce]',
  );
  return 2;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  process.exitCode = main(process.argv.slice(2));
}
