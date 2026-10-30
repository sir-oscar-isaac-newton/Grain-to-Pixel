#!/usr/bin/env node
// Enforces the allowed crate dependency edges of docs/architecture.md section 3.3.
// Usage: node tools/scripts/check-deps/check-deps.mjs [--manifest-path <Cargo.toml>]
// Exit code 1 and one line per forbidden edge when a rule is broken.
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

// Keep in sync with docs/architecture.md section 3.3. Crates that do not exist yet
// (gtp-app, the Tauri shell, tools/benchmark) are listed so their first PR passes.
// When D-01 and Q-01 pick the package names of the Tauri shell and the benchmark tool,
// replace the placeholders below.
export const ALLOWED = {
  'gtp-core': [],
  'gtp-io': ['gtp-core'],
  'gtp-negative': ['gtp-core'],
  'gtp-edit': ['gtp-core'],
  'gtp-catalog': ['gtp-core'],
  'gtp-capture': ['gtp-core'],
  'gtp-ingest': ['gtp-core', 'gtp-io'],
  'gtp-app': [
    'gtp-core',
    'gtp-io',
    'gtp-negative',
    'gtp-edit',
    'gtp-catalog',
    'gtp-ingest',
    'gtp-capture',
  ],
  'grain-to-pixel-desktop': ['gtp-app', 'gtp-core'],
  'gtp-benchmark': ['gtp-app', 'gtp-core'],
};

/** @param {{name: string, dependencies: {name: string}[]}[]} packages workspace members */
export function findViolations(packages, allowed = ALLOWED) {
  const members = new Set(packages.map((p) => p.name));
  const violations = [];
  for (const pkg of packages) {
    const rule = allowed[pkg.name];
    if (!rule) {
      violations.push(
        `unknown workspace package "${pkg.name}": add it to ALLOWED in check-deps.mjs and to docs/architecture.md section 3.3`,
      );
      continue;
    }
    for (const dep of pkg.dependencies) {
      if (!members.has(dep.name)) continue; // external crate, handled by cargo-deny
      // A crate may name itself in [dev-dependencies] to switch on its own test-only feature.
      if (dep.name === pkg.name) continue;
      if (!rule.includes(dep.name)) {
        violations.push(
          `${pkg.name} -> ${dep.name} is not allowed (allowed: ${rule.join(', ') || 'none'})`,
        );
      }
    }
  }
  return violations;
}

export function readPackages(manifestPath) {
  const args = ['metadata', '--format-version', '1', '--no-deps'];
  if (manifestPath) args.push('--manifest-path', manifestPath);
  const res = spawnSync(process.env.CARGO || 'cargo', args, {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  if (res.error || res.status !== 0) {
    throw new Error(`cargo metadata failed: ${res.error?.message ?? res.stderr}`);
  }
  return JSON.parse(res.stdout).packages.map((p) => ({
    name: p.name,
    dependencies: p.dependencies.map((d) => ({ name: d.name })),
  }));
}

function main(argv) {
  const i = argv.indexOf('--manifest-path');
  const manifest = i >= 0 ? argv[i + 1] : undefined;
  let packages;
  try {
    packages = readPackages(manifest);
  } catch (err) {
    console.error(`check-deps: ${err.message}`);
    return 2;
  }
  const violations = findViolations(packages);
  if (violations.length) {
    for (const v of violations) console.error(`FORBIDDEN: ${v}`);
    console.error(`check-deps: ${violations.length} violation(s)`);
    return 1;
  }
  console.log(`check-deps: OK (${packages.length} crates)`);
  return 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  process.exitCode = main(process.argv.slice(2));
}
