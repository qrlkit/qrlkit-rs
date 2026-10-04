const { execFileSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { readFileSync, readdirSync } = require('node:fs');
const path = require('node:path');

const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');

async function publishCrate(version, { request = fetch, run = execFileSync } = {}) {
  const response = await request(`https://crates.io/api/v1/crates/qrlkit/${version}`, {
    headers: { 'User-Agent': 'qrlkit-release (https://github.com/qrlkit/qrlkit-rs)' },
  });
  if (response.status === 404) {
    run('cargo', ['publish', '--locked'], { stdio: 'inherit' });
    return;
  }
  if (!response.ok) throw new Error(`Cannot check crates.io: HTTP ${response.status}`);
  const published = (await response.json()).version;
  if (published?.num !== version || published.yanked) throw new Error('Unexpected or yanked crates.io version');
  run('cargo', ['package', '--locked', '--no-verify'], { stdio: 'inherit' });
  const file = path.resolve(`target/package/qrlkit-${version}.crate`);
  if (hash(readFileSync(file)) !== published.checksum) {
    throw new Error(`qrlkit ${version} already exists on crates.io with different contents; refusing to continue`);
  }
  console.log(`Already published: qrlkit ${version} (checksum verified)`);
}

async function publishRelease(tag, directory, repository, { request = fetch, run = execFileSync } = {}) {
  // Listing also finds drafts left by interrupted uploads; tag lookup only finds published releases.
  let release;
  for (let page = 1; ; page++) {
    const response = await request(`https://api.github.com/repos/${repository}/releases?per_page=100&page=${page}`, {
      headers: { Accept: 'application/vnd.github+json', Authorization: `Bearer ${process.env.GH_TOKEN}` },
    });
    if (!response.ok) throw new Error(`Cannot check GitHub release: HTTP ${response.status}`);
    const releases = await response.json();
    release = releases.find((entry) => entry.tag_name === tag);
    if (release || releases.length < 100) break;
  }
  const assets = release?.assets || [];
  const files = readdirSync(directory).filter((name) => name.startsWith(`qrlkit-${tag.slice(1)}-`) && /\.tar\.gz(?:\.sha256)?$/.test(name));
  if (files.length !== 8) throw new Error(`Expected four binary archives and four checksums, found ${files.length}`);
  const pending = [];
  for (const name of files) {
    const file = path.resolve(directory, name);
    const existing = assets.find((asset) => asset.name === name);
    if (!existing) {
      pending.push(file);
      continue;
    }
    // Older GitHub assets may not have a server-provided digest.
    const digest = existing.digest || `sha256:${hash(run('gh', [
      'api', `repos/${repository}/releases/assets/${existing.id}`, '-H', 'Accept: application/octet-stream',
    ], { maxBuffer: 128 * 1024 * 1024 }))}`;
    if (digest !== `sha256:${hash(readFileSync(file))}`) {
      throw new Error(`Release asset ${name} already exists with different contents; refusing to overwrite`);
    }
  }
  if (!release) {
    run('gh', ['release', 'create', tag, '--repo', repository, '--verify-tag', '--title', tag, '--generate-notes', '--draft'], { stdio: 'inherit' });
  }
  if (pending.length) {
    run('gh', ['release', 'upload', tag, ...pending, '--repo', repository], { stdio: 'inherit' });
  }
  // Keep incomplete releases as drafts, including when uploads are interrupted.
  if (!release || release.draft) {
    run('gh', ['release', 'edit', tag, '--repo', repository, '--draft=false'], { stdio: 'inherit' });
  }
}

module.exports = { publishCrate, publishRelease };
if (require.main === module) {
  const [kind, tag, directory] = process.argv.slice(2);
  if (!tag?.startsWith('v')) throw new Error('Expected release tag starting with v');
  const work = kind === 'crate'
    ? publishCrate(tag.slice(1))
    : kind === 'github' && directory && process.env.GITHUB_REPOSITORY
      ? publishRelease(tag, directory, process.env.GITHUB_REPOSITORY)
      : Promise.reject(new Error('Usage: node .github/release.cjs crate|github <tag> [artifact-directory]'));
  work.catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
