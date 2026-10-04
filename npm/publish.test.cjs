const assert = require('node:assert/strict');
const { createHash } = require('node:crypto');
const { mkdtempSync, readFileSync, rmSync, writeFileSync } = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { publishPackages } = require('./publish.cjs');
const platforms = require('./platforms.cjs');

function fixture(t) {
  const directory = mkdtempSync(path.join(os.tmpdir(), 'qrlkit-publish-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const names = [...Object.keys(platforms).map((key) => `qrlkit-${key}`), 'qrlkit'];
  for (const name of names) writeFileSync(path.join(directory, `${name}-1.2.3.tgz`), name);
  const published = new Map();
  const calls = [];
  const request = async (url) => {
    const name = new URL(url).pathname.split('/')[1];
    return published.has(name)
      ? { ok: true, status: 200, json: async () => published.get(name) }
      : { ok: false, status: 404 };
  };
  const run = (command, args) => {
    calls.push([command, ...args]);
    assert.equal(command, 'npm');
    assert.equal(args[0], 'publish');
    assert(path.isAbsolute(args[1]), 'Publishing must use unambiguous local paths');
    const name = path.basename(args[1], '-1.2.3.tgz');
    published.set(name, { name, version: '1.2.3', dist: {
      integrity: `sha512-${createHash('sha512').update(readFileSync(args[1])).digest('base64')}`,
    } });
  };
  return { directory, names, published, calls, request, run };
}

test('publish native packages before launcher; a complete retry does no writes', async (t) => {
  const f = fixture(t);
  await publishPackages('1.2.3', f.directory, f);
  assert.deepEqual([...f.published.keys()], f.names);
  assert.equal(f.calls.length, 5);
  await publishPackages('1.2.3', f.directory, f);
  assert.equal(f.calls.length, 5);
});

test('resume after a platform publication fails', async (t) => {
  const f = fixture(t);
  let attempts = 0;
  await assert.rejects(publishPackages('1.2.3', f.directory, { ...f, run: (...args) => {
    if (++attempts === 3) throw new Error('Simulated upload failure');
    f.run(...args);
  } }), /upload failure/);
  assert.equal(f.published.size, 2);
  await publishPackages('1.2.3', f.directory, f);
  assert.deepEqual([...f.published.keys()], f.names);
  assert.equal(f.calls.length, 5);
});

test('registry errors, missing files, and conflicting versions never publish', async (t) => {
  const f = fixture(t);
  for (const status of [401, 403, 429, 500]) {
    await assert.rejects(publishPackages('1.2.3', f.directory, { ...f,
      request: async () => ({ ok: false, status }),
    }), new RegExp(`HTTP ${status}`));
  }
  f.published.set('qrlkit', { name: 'qrlkit', version: '1.2.3', dist: { integrity: 'wrong' } });
  await assert.rejects(publishPackages('1.2.3', f.directory, f), /different contents/);
  rmSync(path.join(f.directory, 'qrlkit-1.2.3.tgz'));
  await assert.rejects(publishPackages('1.2.3', f.directory, f), /ENOENT/);
  assert.equal(f.calls.length, 0);
});
