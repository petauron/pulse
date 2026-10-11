// Resolve an existing Release Please draft without creating or changing it.
// The workflow revision and the product source can differ during recovery.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const shaPattern = /^[a-f0-9]{40}$/;
const tagPattern = /^v\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/;

export function validateDraft(draft, tag, expectedSha) {
  assert.match(tag, tagPattern, 'Expected a version tag');
  assert.equal(draft.tagName, tag, 'Release tag mismatch');
  assert.equal(draft.isDraft, true, 'Never rebuild or overwrite a published release');
  assert.equal(draft.author?.login, 'github-actions[bot]', 'Recover only Release Please drafts');
  assert.match(draft.targetCommitish, shaPattern, 'The draft must retain its immutable source SHA');
  if (expectedSha !== undefined) {
    assert.match(expectedSha, shaPattern, 'Expected an immutable source SHA');
    assert.equal(draft.targetCommitish, expectedSha, 'Release Please source mismatch');
  }
  return { tag, sha: draft.targetCommitish };
}

export function provenanceFor(release, env) {
  assert.match(release.tag, tagPattern);
  assert.match(release.sha, shaPattern);
  assert.match(env.GITHUB_SHA, shaPattern);
  assert.equal(env.GITHUB_WORKFLOW_SHA, env.GITHUB_SHA, 'Use the checked workflow revision');
  assert.equal(env.GITHUB_REF, 'refs/heads/main', 'Only protected main may publish');
  assert.ok(['push', 'workflow_dispatch'].includes(env.GITHUB_EVENT_NAME));
  assert.match(env.GITHUB_REPOSITORY, /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/);
  assert.equal(env.GITHUB_SERVER_URL, 'https://github.com');
  assert.equal(env.RUNNER_ENVIRONMENT, 'github-hosted');
  assert.match(env.GITHUB_RUN_ID, /^\d+$/);
  assert.match(env.GITHUB_RUN_ATTEMPT, /^\d+$/);
  assert.match(env.GITHUB_REPOSITORY_ID, /^\d+$/);
  assert.match(env.GITHUB_REPOSITORY_OWNER_ID, /^\d+$/);
  const repository = `${env.GITHUB_SERVER_URL}/${env.GITHUB_REPOSITORY}`;
  const workflowPath = '.github/workflows/release.yml';
  assert.equal(env.GITHUB_WORKFLOW_REF, `${env.GITHUB_REPOSITORY}/${workflowPath}@refs/heads/main`);
  return {
    buildDefinition: {
      // GitHub's attestation API accepts this standard workflow build type.
      buildType: 'https://actions.github.io/buildtypes/workflow/v1',
      externalParameters: {
        workflow: { repository, path: workflowPath, ref: env.GITHUB_REF },
        ...(env.GITHUB_EVENT_NAME === 'workflow_dispatch' ? { inputs: { release_tag: release.tag } } : {}),
      },
      internalParameters: { github: {
        event_name: env.GITHUB_EVENT_NAME,
        repository_id: env.GITHUB_REPOSITORY_ID,
        repository_owner_id: env.GITHUB_REPOSITORY_OWNER_ID,
        runner_environment: env.RUNNER_ENVIRONMENT,
      } },
      resolvedDependencies: [
        { uri: `git+${repository}@${env.GITHUB_REF}`, digest: { gitCommit: env.GITHUB_SHA } },
        { name: 'release-source', uri: `git+${repository}@${release.sha}`, digest: { gitCommit: release.sha } },
      ],
    },
    runDetails: {
      builder: { id: `${env.GITHUB_SERVER_URL}/${env.GITHUB_WORKFLOW_REF}` },
      metadata: { invocationId: `${repository}/actions/runs/${env.GITHUB_RUN_ID}/attempts/${env.GITHUB_RUN_ATTEMPT}` },
    },
  };
}

function main(env) {
  const tag = env.RELEASE_TAG;
  assert.match(tag, tagPattern, 'Expected a version tag');
  assert.ok(['push', 'workflow_dispatch'].includes(env.GITHUB_EVENT_NAME));
  const expectedSha = env.GITHUB_EVENT_NAME === 'push' ? env.EXPECTED_RELEASE_SHA : undefined;
  if (env.GITHUB_EVENT_NAME === 'push') {
    assert.equal(expectedSha, env.GITHUB_SHA, 'New releases must use the triggering commit');
  }
  const draft = JSON.parse(execFileSync('gh', [
    'release', 'view', tag, '--repo', env.GITHUB_REPOSITORY,
    '--json', 'tagName,isDraft,targetCommitish,author',
  ], { encoding: 'utf8' }));
  const release = validateDraft(draft, tag, expectedSha);
  const provenance = provenanceFor(release, env);
  assert.ok(env.GITHUB_OUTPUT);
  appendFileSync(env.GITHUB_OUTPUT, [
    'release_created=true', `release_tag=${release.tag}`, `release_sha=${release.sha}`,
    `provenance=${JSON.stringify(provenance)}`, '',
  ].join('\n'));
  console.log(`Validated draft ${release.tag} at ${release.sha}; workflow ${env.GITHUB_SHA}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.env);
}
