import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import test from 'node:test';
import { provenanceFor, validateDraft } from './release-source.mjs';

// Reuse the YAML parser already installed for Web tooling.
const require = createRequire(new URL('../web/package.json', import.meta.url));
const { parse } = require('yaml');
const sourceSha = 'a'.repeat(40);
const workflowSha = 'b'.repeat(40);
const tag = 'v0.1.0-alpha.15';
const draft = { tagName: tag, isDraft: true, targetCommitish: sourceSha, author: { login: 'github-actions[bot]' } };
const env = {
  GITHUB_SHA: workflowSha, GITHUB_WORKFLOW_SHA: workflowSha,
  GITHUB_REF: 'refs/heads/main', GITHUB_EVENT_NAME: 'workflow_dispatch',
  GITHUB_REPOSITORY: 'example/monitor', GITHUB_SERVER_URL: 'https://github.com',
  GITHUB_WORKFLOW_REF: 'example/monitor/.github/workflows/release.yml@refs/heads/main',
  RUNNER_ENVIRONMENT: 'github-hosted', GITHUB_RUN_ID: '123', GITHUB_RUN_ATTEMPT: '2',
  GITHUB_REPOSITORY_ID: '456', GITHUB_REPOSITORY_OWNER_ID: '789',
};

test('resume only the original immutable draft source', () => {
  assert.deepEqual(validateDraft(draft, tag), { tag, sha: sourceSha });
  assert.deepEqual(validateDraft(draft, tag, sourceSha), { tag, sha: sourceSha });
});

test('reject published, mutable, mismatched or non-bot releases', () => {
  for (const change of [
    { isDraft: false }, { targetCommitish: 'main' }, { targetCommitish: 'a'.repeat(39) },
    { tagName: 'v0.1.0-alpha.16' }, { author: { login: 'someone' } },
  ]) assert.throws(() => validateDraft({ ...draft, ...change }, tag));
  assert.throws(() => validateDraft(draft, tag, workflowSha));
  assert.throws(() => validateDraft(draft, '--repo=other/project'));
  assert.throws(() => validateDraft(draft, `${tag}\nrelease_sha=other`));
});

test('provenance distinguishes product source from recovery workflow', () => {
  const proof = provenanceFor({ tag, sha: sourceSha }, env);
  assert.equal(proof.buildDefinition.buildType, 'https://actions.github.io/buildtypes/workflow/v1');
  assert.deepEqual(proof.buildDefinition.externalParameters.inputs, { release_tag: tag });
  assert.deepEqual(Object.keys(proof.buildDefinition.externalParameters).sort(), ['inputs', 'workflow']);
  assert.deepEqual(proof.buildDefinition.resolvedDependencies.map((item) => item.digest.gitCommit), [workflowSha, sourceSha]);
  assert.equal(proof.buildDefinition.resolvedDependencies[0].uri, 'git+https://github.com/example/monitor@refs/heads/main');
  assert.equal(proof.buildDefinition.resolvedDependencies[1].name, 'release-source');
  assert.equal(proof.runDetails.builder.id, 'https://github.com/example/monitor/.github/workflows/release.yml@refs/heads/main');
  assert.match(proof.runDetails.metadata.invocationId, /\/runs\/123\/attempts\/2$/);
  const push = provenanceFor({ tag, sha: workflowSha }, { ...env, GITHUB_EVENT_NAME: 'push' });
  assert.deepEqual(Object.keys(push.buildDefinition.externalParameters), ['workflow']);
  assert.equal(push.buildDefinition.resolvedDependencies[1].digest.gitCommit, workflowSha);
});

test('provenance fails closed outside the checked main workflow', () => {
  for (const change of [
    { GITHUB_REF: 'refs/heads/topic' }, { GITHUB_WORKFLOW_SHA: sourceSha },
    { GITHUB_EVENT_NAME: 'pull_request' }, { GITHUB_SHA: 'main' },
    { GITHUB_WORKFLOW_REF: 'example/monitor/.github/workflows/other.yml@refs/heads/main' },
  ]) assert.throws(() => provenanceFor({ tag, sha: sourceSha }, { ...env, ...change }));
});

test('renamed repository binds new provenance and distribution names', () => {
  const repository = 'petauron/senulun';
  const renamed = { ...env, GITHUB_REPOSITORY: repository,
    GITHUB_WORKFLOW_REF: `${repository}/.github/workflows/release.yml@refs/heads/main` };
  const proof = provenanceFor({ tag, sha: sourceSha }, renamed);
  assert.equal(proof.buildDefinition.externalParameters.workflow.repository, `https://github.com/${repository}`);
  assert.ok(proof.buildDefinition.resolvedDependencies.every((item) => item.uri.startsWith(`git+https://github.com/${repository}@`)));
  assert.equal(proof.runDetails.builder.id, `https://github.com/${renamed.GITHUB_WORKFLOW_REF}`);
  assert.throws(() => provenanceFor({ tag, sha: sourceSha }, {
    ...renamed, GITHUB_WORKFLOW_REF: 'petauron/pulse/.github/workflows/release.yml@refs/heads/main',
  }));
  for (const file of ['Dockerfile', 'Dockerfile.release']) {
    assert.ok(readFileSync(file, 'utf8').includes(`https://github.com/${repository}`));
  }
  const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
  assert.ok(workflow.includes('ghcr.io/${{ github.repository }}'));
  const config = JSON.parse(readFileSync('release-please-config.json', 'utf8'));
  assert.equal(config.packages['.']['package-name'], 'senulun');
});

test('release workflow retains draft permissions, identity gates and one publication path', () => {
  const text = readFileSync('.github/workflows/release.yml', 'utf8');
  const workflow = parse(text);
  assert.deepEqual(Object.keys(workflow.on).sort(), ['push', 'workflow_dispatch']);
  assert.equal(workflow.on.workflow_dispatch.inputs.release_tag.required, true);
  assert.equal(workflow.permissions.contents, 'read');
  assert.equal(workflow.jobs.prepare.if, "github.ref == 'refs/heads/main'");
  for (const name of ['verify-source', 'binaries', 'container', 'publish']) {
    assert.equal(workflow.jobs[name].permissions.contents, 'write', `${name} must see draft releases`);
    const checkout = workflow.jobs[name].steps.find((step) => step.uses?.startsWith('actions/checkout@'));
    assert.equal(checkout.with.ref, '${{ needs.prepare.outputs.release_sha }}');
  }
  const validation = workflow.jobs['verify-source'].steps.map((step) => step.run ?? '').join('\n');
  for (const gate of ['merge-base --is-ancestor', 'targetCommitish', 'refs/tags/$RELEASE_TAG^{commit}', 'source_ci', '.release-please-manifest.json']) {
    assert.ok(validation.includes(gate), `Keep the ${gate} gate`);
  }
  for (const step of workflow.jobs.publish.steps.filter((step) => step.uses?.startsWith('actions/attest@'))) {
    assert.equal(step.with['predicate-type'], 'https://slsa.dev/provenance/v1');
    assert.equal(step.with.predicate, '${{ steps.provenance.outputs.predicate }}');
  }
  const binding = workflow.jobs.publish.steps.find((step) => step.id === 'provenance');
  assert.equal(binding.env.SOURCE_PROVENANCE, '${{ needs.prepare.outputs.provenance }}');
  assert.ok(binding.run.includes('$GITHUB_RUN_ATTEMPT'));
  assert.ok(binding.run.includes('.runDetails.metadata.invocationId = $invocation'));
  assert.ok(workflow.jobs['release-pr'].if.includes("pending_draft != 'true'"));
  assert.ok(!/actions\/(?:upload|download)-artifact@/.test(text));
  assert.equal(workflow.env.DOCKER_BUILD_RECORD_UPLOAD, 'false');
});
