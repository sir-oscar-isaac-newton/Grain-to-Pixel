import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { findProblems } from './check-progress.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const script = path.join(here, 'check-progress.mjs');

const entry = (over = {}) => ({
  id: 'P-01',
  number: 13,
  title: 'Core domain types',
  branch: 'feat/issue-13-core',
  status: 'completed',
  pr: 89,
  commit: 'f9a911b',
  ...over,
});
const record = (issues) => JSON.stringify({ updated: '2026-10-30', issues }, null, 2);

test('a valid record has no problems', () => {
  const text = record([
    entry(),
    entry({ id: 'P-02', number: 14, status: 'in_progress', pr: null, commit: null }),
  ]);
  assert.deepEqual(findProblems(text), []);
});

test('leftover merge-conflict markers are reported with their lines', () => {
  const text = record([entry()]).replace(
    '"commit": "f9a911b"',
    '"commit": "f9a911b"\n=======\n>>>>>>> origin/develop',
  );
  const problems = findProblems(text);
  assert.equal(problems.length, 2);
  assert.match(problems[0], /merge-conflict marker on line \d+/);
});

test('invalid JSON and a missing issues array are reported', () => {
  assert.match(findProblems('{ "issues": [ }')[0], /not valid JSON/);
  assert.deepEqual(findProblems('{"updated": "x"}'), ['missing the "issues" array']);
});

test('missing fields, unknown status and incomplete completed entries are reported', () => {
  const problems = findProblems(
    record([
      entry({ title: '' }),
      entry({ id: 'X', number: 2, status: 'done' }),
      entry({ id: 'Y', number: 3, pr: null, commit: '' }),
    ]),
  );
  assert.ok(problems.some((p) => p.includes('missing "title"')));
  assert.ok(problems.some((p) => p.includes('unknown status "done"')));
  assert.ok(problems.some((p) => p.includes('completed without a PR number')));
  assert.ok(problems.some((p) => p.includes('completed without a commit')));
});

test('duplicate ids and numbers are reported', () => {
  const problems = findProblems(record([entry(), entry({ title: 'again' })]));
  assert.ok(problems.some((p) => p.includes('duplicate id')));
  assert.ok(problems.some((p) => p.includes('duplicate number 13')));
});

test('this repository passes (exit 0)', () => {
  const res = spawnSync(process.execPath, [script], { encoding: 'utf8' });
  assert.equal(res.status, 0, res.stderr);
  assert.match(res.stdout, /check-progress: OK/);
});

test('a broken file fails (exit 1) and names the problem', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'check-progress-'));
  try {
    const file = path.join(dir, 'progress.md');
    writeFileSync(file, record([entry()]).replace('    }', '=======\n    }'));
    const res = spawnSync(process.execPath, [script, '--file', file], { encoding: 'utf8' });
    assert.equal(res.status, 1);
    assert.match(res.stderr, /merge-conflict marker/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
