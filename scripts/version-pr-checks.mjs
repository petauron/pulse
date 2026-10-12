import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

// Reuse real checks on the exact version PR head, including failures that need
// a source fix. An approval-blocked or cancelled run supplies no complete check.
export function needsDispatch(runs, workflow, sha) {
  assert.match(sha, /^[a-f0-9]{40}$/);
  return !runs.some((run) =>
    run.path === `.github/workflows/${workflow}` && run.head_sha === sha &&
    ['pull_request', 'workflow_dispatch'].includes(run.event) &&
    run.conclusion !== 'action_required' && (
      ['queued', 'pending', 'requested', 'in_progress'].includes(run.status) ||
      (run.status === 'completed' && ['success', 'failure', 'timed_out', 'startup_failure'].includes(run.conclusion))
    ));
}

function main(env) {
  assert.match(env.GITHUB_REPOSITORY, /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/);
  const releasePr = JSON.parse(env.RELEASE_PR);
  assert.ok(Number.isSafeInteger(releasePr.number) && releasePr.number > 0);
  const gh = (args) => execFileSync('gh', args, { encoding: 'utf8' });
  const pr = JSON.parse(gh(['pr', 'view', String(releasePr.number), '--repo', env.GITHUB_REPOSITORY,
    '--json', 'headRefName,headRefOid']));
  assert.equal(pr.headRefName, releasePr.headBranchName);
  const { workflow_runs: runs } = JSON.parse(gh(['api', '--method', 'GET',
    `repos/${env.GITHUB_REPOSITORY}/actions/runs`, '-f', `head_sha=${pr.headRefOid}`, '-f', 'per_page=100']));
  for (const workflow of ['ci.yml', 'dependency-review.yml']) {
    if (needsDispatch(runs, workflow, pr.headRefOid)) {
      gh(['workflow', 'run', workflow, '--repo', env.GITHUB_REPOSITORY, '--ref', pr.headRefName]);
      console.log(`Started ${workflow} for version PR #${releasePr.number} at ${pr.headRefOid}`);
    } else {
      console.log(`Reusing ${workflow} for version PR #${releasePr.number} at ${pr.headRefOid}`);
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main(process.env);
