import assert from 'node:assert/strict';
import test from 'node:test';
import { needsDispatch } from './version-pr-checks.mjs';

const sha = 'a'.repeat(40);
const run = { path: '.github/workflows/ci.yml', head_sha: sha, event: 'pull_request', status: 'in_progress', conclusion: null };

test('reuse active native or dispatched checks on the exact version PR head', () => {
  for (const event of ['pull_request', 'workflow_dispatch']) {
    for (const status of ['queued', 'pending', 'requested', 'in_progress']) {
      assert.equal(needsDispatch([{ ...run, event, status }], 'ci.yml', sha), false);
    }
  }
  assert.equal(needsDispatch([], 'ci.yml', sha), true);
});

test('retain completed results, including real failures, instead of rerunning them automatically', () => {
  for (const conclusion of ['success', 'failure', 'timed_out', 'startup_failure']) {
    assert.equal(needsDispatch([{ ...run, status: 'completed', conclusion }], 'ci.yml', sha), false);
  }
});

test('approval-blocked, cancelled, skipped, unrelated or stale runs cannot supply the checks', () => {
  for (const change of [
    { status: 'completed', conclusion: 'action_required' },
    { status: 'action_required', conclusion: null },
    { status: 'completed', conclusion: 'cancelled' },
    { status: 'completed', conclusion: 'skipped' },
    { head_sha: 'b'.repeat(40) }, { path: '.github/workflows/release.yml' }, { event: 'push' },
  ]) assert.equal(needsDispatch([{ ...run, ...change }], 'ci.yml', sha), true);
  assert.equal(needsDispatch([run], 'dependency-review.yml', sha), true);
  assert.throws(() => needsDispatch([run], 'ci.yml', 'main'));
});
