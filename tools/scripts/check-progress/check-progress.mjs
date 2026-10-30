#!/usr/bin/env node
// Fails when docs/progress.md is not a valid progress record: leftover merge-conflict markers,
// invalid JSON, missing fields, an unknown status, duplicate ids or numbers, or a completed issue
// without its PR and commit. The format is described in docs/workflow.md section 10.
// Usage: node tools/scripts/check-progress/check-progress.mjs [--file <path>]
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const DEFAULT_FILE = path.join(root, 'docs/progress.md');
const STATUSES = new Set(['in_progress', 'completed']);
const TEXT_FIELDS = ['id', 'title', 'branch'];

/** Every problem found in the text of a progress record; empty when it is valid. */
export function findProblems(text) {
  const lines = text.replace(/\r\n/g, '\n').split('\n');
  const markers = lines
    .map((line, i) => [line, i + 1])
    .filter(([line]) => /^(<{7}|={7}|>{7})( |$)/.test(line))
    .map(([, n]) => `merge-conflict marker on line ${n}`);
  if (markers.length) return markers;

  let record;
  try {
    record = JSON.parse(text);
  } catch (err) {
    return [`not valid JSON: ${err.message}`];
  }
  if (!record || !Array.isArray(record.issues)) return ['missing the "issues" array'];

  const problems = [];
  const seenIds = new Set();
  const seenNumbers = new Set();
  record.issues.forEach((issue, i) => {
    const where = `issue ${i + 1} (${issue?.id ?? 'no id'})`;
    for (const field of TEXT_FIELDS) {
      if (typeof issue?.[field] !== 'string' || !issue[field])
        problems.push(`${where}: missing "${field}"`);
    }
    if (!Number.isInteger(issue?.number)) problems.push(`${where}: "number" must be an integer`);
    if (!STATUSES.has(issue?.status)) problems.push(`${where}: unknown status "${issue?.status}"`);
    if (issue?.status === 'completed') {
      if (!Number.isInteger(issue.pr)) problems.push(`${where}: completed without a PR number`);
      if (typeof issue.commit !== 'string' || !issue.commit)
        problems.push(`${where}: completed without a commit`);
    }
    if (seenIds.has(issue?.id)) problems.push(`${where}: duplicate id`);
    if (seenNumbers.has(issue?.number)) problems.push(`${where}: duplicate number ${issue.number}`);
    seenIds.add(issue?.id);
    seenNumbers.add(issue?.number);
  });
  return problems;
}

function main(argv) {
  const i = argv.indexOf('--file');
  const file = i >= 0 ? path.resolve(argv[i + 1] ?? '') : DEFAULT_FILE;
  let text;
  try {
    text = readFileSync(file, 'utf8');
  } catch (err) {
    console.error(`check-progress: cannot read ${file}: ${err.message}`);
    return 2;
  }
  const problems = findProblems(text);
  if (problems.length) {
    for (const p of problems) console.error(`INVALID: ${p}`);
    console.error(`check-progress: ${problems.length} problem(s) in ${path.relative(root, file)}`);
    return 1;
  }
  console.log(`check-progress: OK (${JSON.parse(text).issues.length} issues)`);
  return 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  process.exitCode = main(process.argv.slice(2));
}
