const assert = require('node:assert/strict');
const { createHash } = require('node:crypto');
const { mkdirSync, mkdtempSync, rmSync, writeFileSync } = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { publishCrate, publishRelease } = require('./release.cjs');
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');

test('crate publish distinguishes missing, matching, conflicting and unavailable versions', async (t) => {
  const previous = process.cwd();
  const directory = mkdtempSync(path.join(os.tmpdir(), 'qrlkit-crate-test-'));
  process.chdir(directory);
  t.after(() => { process.chdir(previous); rmSync(directory, { recursive: true, force: true }); });
  const calls = [];
  const run = (command, args) => {
    calls.push([command, ...args]);
    if (args[0] === 'package') {
      mkdirSync('target/package', { recursive: true });
      writeFileSync('target/package/qrlkit-1.2.3.crate', 'crate contents');
    }
  };
  await publishCrate('1.2.3', { run, request: async () => ({ status: 404 }) });
  assert.deepEqual(calls.shift(), ['cargo', 'publish', '--locked']);
  const version = { num: '1.2.3', yanked: false, checksum: hash('crate contents') };
  const request = async () => ({ ok: true, status: 200, json: async () => ({ version }) });
  await publishCrate('1.2.3', { run, request });
  assert.deepEqual(calls.shift(), ['cargo', 'package', '--locked', '--no-verify']);
  version.checksum = 'different';
  await assert.rejects(publishCrate('1.2.3', { run, request }), /different contents/);
  calls.length = 0;
  version.yanked = true;
  await assert.rejects(publishCrate('1.2.3', { run, request }), /yanked/);
  for (const status of [403, 429, 500]) {
    await assert.rejects(publishCrate('1.2.3', { run, request: async () => ({ status, ok: false }) }), /Cannot check/);
  }
  assert.equal(calls.length, 0);
});

test('GitHub release resumes interrupted drafts, skips matching assets and rejects conflicts', async (t) => {
  const directory = mkdtempSync(path.join(os.tmpdir(), 'qrlkit-release-test-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  for (const platform of ['linux-x86_64', 'linux-aarch64', 'macos-x86_64', 'macos-aarch64']) {
    for (const suffix of ['.tar.gz', '.tar.gz.sha256']) {
      const name = `qrlkit-1.2.3-${platform}${suffix}`;
      writeFileSync(path.join(directory, name), name);
    }
  }
  let release = null;
  let failUpload = true;
  const calls = [];
  const request = async (url) => {
    assert.equal(new URL(url).pathname, '/repos/qrlkit/qrlkit-rs/releases');
    // Exercise pagination as well as discovery of both draft and published releases.
    const releases = new URL(url).searchParams.get('page') === '1'
      ? Array.from({ length: 100 }, (_, index) => ({ tag_name: `v0.0.${index}` }))
      : release ? [release] : [];
    return { ok: true, status: 200, json: async () => releases };
  };
  const run = (command, args) => {
    calls.push([command, ...args]);
    assert.equal(command, 'gh');
    if (args[0] === 'api') return Buffer.from(release.assets.find((asset) => args[1].endsWith(`/${asset.id}`)).name);
    if (args[1] === 'create') release = { tag_name: 'v1.2.3', draft: true, assets: [] };
    if (args[1] === 'upload') {
      for (const file of args.slice(3, -2)) {
        const name = path.basename(file);
        release.assets.push({ id: release.assets.length + 1, name, digest: `sha256:${hash(name)}` });
        if (failUpload && release.assets.length === 3) {
          failUpload = false;
          throw new Error('Simulated upload failure');
        }
      }
    }
    if (args[1] === 'edit') release.draft = false;
  };
  await assert.rejects(publishRelease('v1.2.3', directory, 'qrlkit/qrlkit-rs', { request, run }), /upload failure/);
  assert.equal(calls[0][2], 'create');
  assert(calls[0].includes('--draft'));
  assert.equal(calls[1].slice(4, -2).length, 8);
  assert.equal(calls.length, 2);
  assert.equal(release.draft, true);
  assert.equal(release.assets.length, 3);
  calls.length = 0;
  await publishRelease('v1.2.3', directory, 'qrlkit/qrlkit-rs', { request, run });
  assert.equal(calls.length, 2);
  assert.equal(calls[0][2], 'upload');
  assert.equal(calls[0].slice(4, -2).length, 5);
  assert.equal(calls[1][2], 'edit');
  assert(calls[1].includes('--draft=false'));
  assert.equal(release.draft, false);
  assert.equal(release.assets.length, 8);
  calls.length = 0;
  await publishRelease('v1.2.3', directory, 'qrlkit/qrlkit-rs', { request, run });
  assert.equal(calls.length, 0);
  // Older assets without a digest are downloaded and compared, never overwritten.
  delete release.assets[0].digest;
  await publishRelease('v1.2.3', directory, 'qrlkit/qrlkit-rs', { request, run });
  assert.equal(calls.length, 1);
  assert.equal(calls[0][1], 'api');
  calls.length = 0;
  release.assets[0].digest = 'different';
  await assert.rejects(publishRelease('v1.2.3', directory, 'qrlkit/qrlkit-rs', { request, run }), /different contents/);
  for (const status of [403, 404, 429, 500]) {
    await assert.rejects(publishRelease('v1.2.3', directory, 'qrlkit/qrlkit-rs', { run,
      request: async () => ({ ok: false, status }),
    }), /Cannot check/);
  }
  assert.equal(calls.length, 0);
});
