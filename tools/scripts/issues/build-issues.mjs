#!/usr/bin/env node
// Builds docs/issues.md (human-readable list + JSON array for bulk import) from issues.src.txt.
// Usage: node tools/scripts/issues/build-issues.mjs [--check]
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../../..');
const srcPath = path.join(here, 'issues.src.txt');
const outPath = path.join(root, 'docs', 'issues.md');

const TAGS = {
  T: { label: 'area:tooling', name: 'Tooling, CI and release', paths: '.github/, tools/, root configs' },
  R: { label: 'area:research', name: 'Research spikes', paths: 'docs/research/, decisions/' },
  P: { label: 'area:pipeline', name: 'Image pipeline', paths: 'crates/gtp-core, gtp-io, gtp-negative, gtp-edit, gtp-app (pipeline, develop, export)' },
  L: { label: 'area:library', name: 'Library and data', paths: 'crates/gtp-catalog, gtp-app services (library, import, tags, search, jobs)' },
  H: { label: 'area:hardware', name: 'Hardware ingest and capture', paths: 'crates/gtp-ingest, crates/gtp-capture' },
  D: { label: 'area:desktop-shell', name: 'Desktop shell (Tauri)', paths: 'apps/desktop/src-tauri' },
  U: { label: 'area:ui', name: 'User interface', paths: 'apps/desktop/src, packages/ui' },
  Q: { label: 'area:quality', name: 'Quality, docs and release verification', paths: 'tools/benchmark, docs/, samples/' },
};
const MILESTONES = [
  ['M0', 'M0 Foundations', 'App launches on Windows x64 and macOS; hardware spikes answered'],
  ['M1', 'M1 Pipeline on files', 'Quality metric calibrated; budgets met on fixtures'],
  ['M2', 'M2 Library', 'Create library, import 36 frames, tag, search under budget'],
  ['M3', 'M3 Develop UI', 'Full file-based workflow usable end to end'],
  ['M4', 'M4 LS-50 ingest', 'Full-roll scan with IR on each OS'],
  ['M5', 'M5 RGB triplets (files)', 'D600 NEF triplet converts correctly'],
  ['M6', 'M6 Guided RGB capture', 'Roll captured with Scanlight + D600 on macOS, then Windows'],
  ['M7', 'M7 MVP release v0.1.0', 'Budgets and quality metric met; release checklist passed'],
];
const TYPES = ['feature', 'chore', 'test', 'docs', 'spike'];
const LABELS = [
  ...Object.values(TAGS).map((t) => [t.label, '1d76db', t.name]),
  ...TYPES.map((t) => [`type:${t}`, '5319e7', `Issue type: ${t}`]),
  ['size:S', 'c2e0c6', 'About half a day'],
  ['size:M', 'fbca04', 'About a day'],
  ['needs:hw', 'd93f0b', 'Needs real hardware; the human runs it and reports'],
  ['needs:data', 'd93f0b', 'Needs files only the human has'],
  ['needs:mac', 'd93f0b', 'Needs a macOS check until CI covers it'],
  ['needs:human', 'd93f0b', 'Needs an action only the human can do'],
  ['adr', '0e8a16', 'Requires or updates an ADR'],
  ['status:proposed', 'bfdadc', 'Proposal posted, waiting for approval'],
  ['status:in-progress', 'bfdadc', 'Approved and being implemented'],
  ['status:in-review', 'bfdadc', 'PR open'],
  ['status:blocked', 'b60205', 'Blocked; reason in the last comment'],
];

const AUDIT = [
  'Re-tagged every issue by repo area (T, R, P, L, H, D, U, Q) and numbered each tag from 01 in dependency order; the previous GTP-0NN IDs are kept as `legacy`.',
  'Split issues that mixed backend and UI so each issue touches one area: GTP-055 into L-08 (recipe save) and U-06 (apply-settings UI); GTP-056 into P-12 (export job) and U-11 (export dialog); GTP-064 into U-08 (scanner UI) and U-12 (dust panel); GTP-072 into H-06 (triplet import job) and U-09 (triplet UI).',
  'Added L-09: thumbnails are persisted and must be regenerated after a recipe changes (spec 5.1); the old list never did this.',
  'Added L-10: one shared service that estimates the roll film base from the first ingested frame; import, scan and capture all need it (spec 4.2).',
  'Corrected dependencies: JobManager lives in gtp-app so L-01 depends on P-10; scan, triplet import and capture jobs depend on the import commit path (L-05) and L-10; the capture orchestrator depends on the triplet import path (H-06); U-12 depends on the IR stage (P-13).',
  'Moved cross-cutting requirements into acceptance criteria: `GTP_FAKE_HW=1` selects fixture/fake devices (H-01, D-04), the light is switched off on app exit and view unmount (D-04, U-10), the update check can be disabled (T-06), the macOS-only `tether` feature is off for Windows builds (T-05, H-09).',
  'Every issue now has one area, one type, one size, an estimate, labels, explicit dependencies and a self-contained AI prompt; the upload order is a verified topological order, so every dependency is created before its dependents.',
  'Checked consistency: no dependency points to a later milestone; every crate and top-level folder in structure.md and architecture.md is owned by at least one issue; each issue has one primary area (a thin src-tauri command may accompany a backend service issue).',
];

// ---------- parse ----------
const KEYS = new Set(['title', 'milestone', 'type', 'size', 'estimate', 'legacy', 'deps', 'needs', 'adr', 'labels', 'spec', 'desc', 'ac', 'tech']);
function parse(text) {
  const list = [];
  let cur = null;
  let key = null;
  for (const line of text.split('\n')) {
    let m = line.match(/^## ([A-Z]-\d{2})\s*$/);
    if (m) {
      cur = { id: m[1], raw: {} };
      list.push(cur);
      key = null;
      continue;
    }
    if (!cur) continue;
    m = line.match(/^([a-z]+):\s?(.*)$/);
    if (m && KEYS.has(m[1])) {
      key = m[1];
      cur.raw[key] = [m[2]];
      continue;
    }
    if (key) cur.raw[key].push(line);
  }
  return list.map(({ id, raw }) => {
    const one = (k) => (raw[k] || []).join(' ').replace(/\s+/g, ' ').trim();
    const csv = (k) => one(k).split(',').map((s) => s.trim()).filter(Boolean);
    const ac = [];
    for (const l of raw.ac || []) {
      if (l.startsWith('- ')) ac.push(l.slice(2).trim());
      else if (l.trim() && ac.length) ac[ac.length - 1] += ' ' + l.trim();
    }
    return {
      id,
      title: one('title'),
      milestone: one('milestone'),
      type: one('type'),
      size: one('size'),
      estimate: one('estimate'),
      legacy: one('legacy'),
      deps: csv('deps'),
      needs: csv('needs'),
      adr: one('adr') === 'yes',
      extraLabels: csv('labels'),
      spec: one('spec'),
      desc: one('desc'),
      ac,
      tech: (raw.tech || []).join('\n').trim(),
    };
  });
}

// ---------- validate + order ----------
function validate(issues) {
  const errs = [];
  const ids = new Set();
  const msIndex = Object.fromEntries(MILESTONES.map((m, i) => [m[0], i]));
  for (const i of issues) {
    if (ids.has(i.id)) errs.push(`duplicate id ${i.id}`);
    ids.add(i.id);
    if (!TAGS[i.id[0]]) errs.push(`${i.id}: unknown tag`);
    for (const f of ['title', 'milestone', 'type', 'size', 'estimate', 'desc', 'tech']) if (!i[f]) errs.push(`${i.id}: missing ${f}`);
    if (!i.ac.length) errs.push(`${i.id}: no acceptance criteria`);
    if (!TYPES.includes(i.type)) errs.push(`${i.id}: bad type ${i.type}`);
    if (!['S', 'M'].includes(i.size)) errs.push(`${i.id}: bad size ${i.size}`);
    if (!(i.milestone in msIndex)) errs.push(`${i.id}: bad milestone ${i.milestone}`);
    for (const n of i.needs) if (!['hw', 'data', 'mac', 'human'].includes(n)) errs.push(`${i.id}: bad needs ${n}`);
  }
  const byId = Object.fromEntries(issues.map((i) => [i.id, i]));
  for (const i of issues) {
    for (const d of i.deps) {
      if (!byId[d]) errs.push(`${i.id}: unknown dependency ${d}`);
      else if (msIndex[byId[d].milestone] > msIndex[i.milestone]) errs.push(`${i.id}: depends on later milestone issue ${d}`);
    }
  }
  return errs;
}
function order(issues) {
  const placed = [];
  const done = new Set();
  const rest = [...issues];
  while (rest.length) {
    const idx = rest.findIndex((i) => i.deps.every((d) => done.has(d)));
    if (idx < 0) throw new Error('dependency cycle among: ' + rest.map((i) => i.id).join(', '));
    const [i] = rest.splice(idx, 1);
    placed.push(i);
    done.add(i.id);
  }
  return placed;
}

// ---------- compose ----------
const ms = Object.fromEntries(MILESTONES.map((m) => [m[0], m[1]]));
const fullTitle = (i) => `${i.id} – ${i.title}`;
function labelsFor(i) {
  const set = [TAGS[i.id[0]].label, `type:${i.type}`, `size:${i.size}`, ...i.needs.map((n) => `needs:${n}`)];
  if (i.adr) set.push('adr');
  for (const l of i.extraLabels) if (l !== 'spike' && l !== 'adr') set.push(l);
  return [...new Set(set)];
}
function depLine(i, byId) {
  return i.deps.length ? i.deps.map((d) => `${d} – ${byId[d].title}`).join('; ') : 'none';
}
function prompt(i, byId) {
  const needs = i.needs.length
    ? 'Needs from the human: ' + i.needs.map((n) => ({ hw: 'real hardware run and report', data: 'files only the human has', mac: 'a macOS check', human: 'an action only the human can do' }[n])).join('; ') + '. Prepare exact steps and a checklist; never tick these items yourself.'
    : 'Needs from the human: approval of your proposal.';
  return [
    `You are working on the project "Grain to Pixel" in the repository at the current working directory. Follow docs/workflow.md: read this issue, post a short proposal (understanding, approach, files, tests, docs, risks, open questions) and STOP for my explicit approval before writing code. After approval implement with tests and docs, run the CI gates from docs/workflow.md section 8, open a PR that closes the GitHub issue, then report and ask for the next step.`,
    ``,
    `PROJECT CONTEXT (from docs/product/idea.md)`,
    `- Grain to Pixel is a personal Windows 10/11 and macOS desktop app for close friends only, no monetization: an all-in-one tool to scan, convert, edit and organize film negatives (commercial licences are prohibitive).`,
    `- Core workflow: ingest via Nikon Coolscan LS-50 (RGB + IR; the author's modded scanner scans whole rolls of up to 40 frames) or via separate R/G/B exposures shot with a Nikon D600 (NEF) under a Scanlight V4 light, adding tags -> determine frame area and measure film base -> convert to positive -> edit colour balance, tone, geometry, IR/ICE dust removal -> searchable tagged archive.`,
    `- Non-goals: masking, AI generation, advanced retouching. Success: activation, and results comparable to Nikon Scan 4 / Trichroma Lab. Risk: little colour-negative sample data. Constraint: built with Claude Pro, so keep the session narrow and read only what this issue needs.`,
    ``,
    `TECHNICAL CONTEXT (docs/specification.md, docs/architecture.md, docs/structure.md, docs/convention.md)`,
    `- Stack: Tauri 2; Rust workspace in crates/ (gtp-core, gtp-io, gtp-negative, gtp-edit, gtp-catalog, gtp-ingest, gtp-capture, gtp-app); React 18 + TypeScript (strict) + Vite UI in apps/desktop with Zustand, Radix UI and CSS modules; generated packages/ipc-types (tauri-specta); SQLite catalog (rusqlite bundled, FTS5, forward-only migrations); tracing logs; i18n via t() with en.json only.`,
    `- Architecture: feature crates depend on gtp-core (gtp-ingest also on gtp-io) and never on each other; gtp-app is the application layer that combines them (services, JobManager, pipeline); src-tauri is a thin adapter (validate, call a gtp-app service, map errors to AppError); the UI never touches pixels and receives rendered bytes. Long work runs as cancellable jobs with progress events. Hardware sits behind traits (ScannerSource, LightSource, CameraSource) with fakes.`,
    `- Pipeline (spec 4.2) fixed order: load -> IR dust -> crop -> film-base normalize -> invert -> colour -> tone -> geometry -> output transform; internal f32 linear with Rec.2020 primaries; originals immutable, all edits in the versioned Recipe.`,
    `- Repo layout: apps/desktop (src React UI, src-tauri shell), crates/*, packages/{ipc-types,ui}, tools/{benchmark,scripts}, samples/{public,private}, docs/, decisions/ (ADRs), rules/, prompts/.`,
    `- Conventions: no unwrap/expect/unsafe/println in committed Rust, no console.log or any in TS; validate at boundaries; AppError codes; tracing spans; tests are mandatory (unit, golden, fakes, component); Conventional Commits; performance budgets in spec section 11; security in spec section 9.`,
    ``,
    `ISSUE ${fullTitle(i)}`,
    `Milestone: ${ms[i.milestone]} | Area: ${TAGS[i.id[0]].name} | Type: ${i.type} | Size: ${i.size} (${i.estimate})`,
    `Spec references: ${i.spec || 'see docs/specification.md'}`,
    `Dependencies (must be Done first; if any is not, stop and tell me): ${depLine(i, byId)}`,
    ``,
    `DESCRIPTION`,
    i.desc,
    ``,
    `TECHNICAL DETAILS`,
    i.tech,
    ``,
    `ACCEPTANCE CRITERIA`,
    ...i.ac.map((a) => `- ${a}`),
    ``,
    needs,
    `Definition of Done: docs/workflow.md section 11. Update the docs this change makes stale. Do not widen scope; propose follow-up issues instead.`,
  ].join('\n');
}
function body(i, byId) {
  return [
    `## Description`,
    i.desc,
    ``,
    `## Acceptance criteria`,
    ...i.ac.map((a) => `- [ ] ${a}`),
    ``,
    `## Estimate`,
    `${i.estimate} (size ${i.size})`,
    ``,
    `## Dependencies`,
    ...(i.deps.length ? i.deps.map((d) => `- {{dep:${d}}} ${d} – ${byId[d].title}`) : ['- none']),
    ``,
    `## References`,
    `- Spec: ${i.spec || 'docs/specification.md'}`,
    `- Milestone: ${ms[i.milestone]}; area: ${TAGS[i.id[0]].name}; previous ID: ${i.legacy || 'none'}`,
  ].join('\n');
}

function main() {
  const text = fs.readFileSync(srcPath, 'utf8').replace(/\r\n/g, '\n');
  const parsed = parse(text);
  const errs = validate(parsed);
  if (errs.length) {
    console.error('Validation failed:\n- ' + errs.join('\n- '));
    process.exit(1);
  }
  const issues = order(parsed);
  const byId = Object.fromEntries(issues.map((i) => [i.id, i]));
  const records = issues.map((i) => {
    const p = prompt(i, byId);
    return {
      id: i.id,
      title: fullTitle(i),
      description: i.desc,
      acceptance_criteria: i.ac,
      estimate: i.estimate,
      labels: labelsFor(i),
      milestone: ms[i.milestone],
      dependencies: i.deps,
      prompt: p,
      body: body(i, byId),
    };
  });
  if (process.argv.includes('--check')) {
    console.log(`OK: ${records.length} issues, order valid.`);
    return;
  }

  const out = [];
  out.push('# Issues', '');
  out.push('> Generated by `node tools/scripts/issues/build-issues.mjs` from `tools/scripts/issues/issues.src.txt`. Edit the source file, not this one. Upload instructions: [issues_upload_instructions.md](issues_upload_instructions.md).', '');
  out.push(`${records.length} issues across ${MILESTONES.length} milestones. Rules for working through them: [workflow.md](workflow.md). Each issue below has a copy-pastable AI prompt; the JSON array at the end is the bulk-import payload for \`gh\`.`, '');
  out.push('## How progress is tracked', '');
  out.push('- **GitHub Issues are the live tracker.** Open/closed state, the `status:*` labels (`proposed`, `in-progress`, `in-review`, `blocked`), the milestone and the issue comments replace the old status column and progress log.');
  out.push('- This file is the **definition** of the backlog. When scope changes, edit `issues.src.txt`, regenerate this file, and update the affected GitHub issue.');
  out.push('- Upload order is a verified topological order: every dependency appears before its dependents.', '');
  out.push('## Audit summary', '');
  AUDIT.forEach((a, n) => out.push(`${n + 1}. ${a}`));
  out.push('');
  out.push('## Tags, labels, milestones', '', '| Tag | Area | Covers |', '|---|---|---|');
  for (const [k, t] of Object.entries(TAGS)) out.push(`| \`${k}-NN\` | ${t.name} | ${t.paths} |`);
  out.push('', 'Labels: `area:*` (one per issue), `type:*`, `size:S|M` (S = 0.5d, M = 1d), `needs:hw|data|mac|human` (the assistant prepares steps; the human acts and reports), `adr`, and `status:*`.', '');
  out.push('| Milestone | Exit criteria |', '|---|---|');
  for (const m of MILESTONES) out.push(`| ${m[1]} | ${m[2]} |`);
  out.push('');
  out.push('## Mapping from the previous IDs', '', '| Previous | New |', '|---|---|');
  const map = new Map();
  for (const i of records) {
    const src = byId[i.id].legacy;
    const base = (src.match(/GTP-\d+/) || [null])[0];
    const key = base || 'new (audit)';
    if (!map.has(key)) map.set(key, []);
    map.get(key).push(i.id);
  }
  [...map.entries()].sort((a, b) => a[0].localeCompare(b[0])).forEach(([k, v]) => out.push(`| ${k} | ${v.join(', ')} |`));
  out.push('');
  out.push('## Index (upload order)', '', '| # | ID | Title | Milestone | Estimate | Depends on | Labels |', '|---|---|---|---|---|---|---|');
  records.forEach((r, n) => {
    const i = byId[r.id];
    out.push(`| ${n + 1} | ${r.id} | ${i.title.replace(/\|/g, '\\|')} | ${i.milestone} | ${i.estimate} | ${i.deps.join(', ') || '-'} | ${r.labels.filter((l) => !l.startsWith('size:') && !l.startsWith('type:')).join(', ')} |`);
  });
  out.push('');
  const totalDays = issues.reduce((s, i) => s + parseFloat(i.estimate), 0);
  out.push(`Total estimate: ${totalDays} days (${issues.filter((i) => i.size === 'S').length} small, ${issues.filter((i) => i.size === 'M').length} medium).`, '');
  out.push('---', '', '## Issues', '');
  for (const r of records) {
    const i = byId[r.id];
    out.push(`### ${r.title}`, '');
    out.push(`- Milestone: ${r.milestone} | Estimate: ${r.estimate} (size ${i.size}) | Type: ${i.type}`);
    out.push(`- Labels: ${r.labels.join(', ')}`);
    out.push(`- Depends on: ${i.deps.join(', ') || 'none'}`);
    out.push(`- Previous ID: ${i.legacy || 'none'} | Spec: ${i.spec || '-'}`, '');
    out.push('**Description**', '', i.desc, '', '**Acceptance criteria**', '');
    i.ac.forEach((a) => out.push(`- [ ] ${a}`));
    out.push('', '<details><summary>Copy-paste AI prompt</summary>', '', '```text', r.prompt, '```', '', '</details>', '');
  }
  out.push('---', '', '## JSON for bulk import', '');
  out.push('A valid JSON array, in upload order. Fields: `id`, `title`, `description`, `acceptance_criteria`, `estimate`, `labels`, `milestone`, `dependencies`, `prompt`, `body`. `body` (description, criteria, estimate, dependencies, references) contains `{{dep:ID}}` placeholders that the upload script replaces with GitHub issue numbers; the script appends `prompt` to the body in a collapsible section so each GitHub issue is self-contained.', '');
  out.push('<!-- issues-json:start -->', '```json', JSON.stringify(records, null, 2), '```', '<!-- issues-json:end -->', '');
  fs.writeFileSync(outPath, out.join('\n'), 'utf8');

  // metadata for the upload script (labels and milestones)
  fs.writeFileSync(
    path.join(root, 'tools', 'scripts', 'github', 'repo-setup.json'),
    JSON.stringify({ labels: LABELS.map(([name, color, description]) => ({ name, color, description })), milestones: MILESTONES.map(([, title, description]) => ({ title, description })) }, null, 2),
    'utf8',
  );
  console.log(`Wrote ${path.relative(root, outPath)} with ${records.length} issues (${totalDays} days).`);
}
main();
