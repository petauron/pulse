import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';

const root = new URL('../', import.meta.url);
const read = path => readFileSync(new URL(path, root), 'utf8');

function sourceFiles(path) {
  return readdirSync(new URL(path, root), { withFileTypes: true }).flatMap(entry => {
    const child = `${path}/${entry.name}`;
    return entry.isDirectory() ? sourceFiles(child) : /\.(vue|ts|rs)$/.test(entry.name) ? [child] : [];
  });
}

test('external traffic configuration uses the Senulun environment key end to end', () => {
  for (const path of [
    'crates/senulun-service/src/main.rs',
    'crates/senulun-service/src/lib.rs',
    'web/src/components/admin/AdminTrafficSources.vue',
    '.env.example',
    'docs/external-traffic.md',
  ]) {
    const text = read(path);
    assert.ok(text.includes('SENULUN_TRAFFIC_KEY_FILE'), path);
    assert.ok(!text.includes('PULSE_TRAFFIC_KEY_FILE'), path);
  }
});

test('Web sources do not reintroduce old product symbols or browser namespaces', () => {
  for (const path of sourceFiles('web/src')) {
    assert.doesNotMatch(read(path), /PulseApi|pulse:|https:\/\/pulse\.example/, path);
  }
});

test('runtime source uses only the Senulun environment namespace', () => {
  for (const path of sourceFiles('crates')) {
    assert.doesNotMatch(read(path), /\bPULSE_[A-Z0-9_]+\b/, path);
  }
});
