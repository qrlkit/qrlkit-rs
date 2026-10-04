const { createHash } = require('node:crypto');
const { readFileSync } = require('node:fs');
const { execFileSync } = require('node:child_process');
const path = require('node:path');
const platforms = require('./platforms.cjs');

async function publishPackages(version, directory, { request = fetch, run = execFileSync } = {}) {
  const names = [...Object.keys(platforms).map((platform) => `qrlkit-${platform}`), 'qrlkit'];
  // Validate all files and existing versions before the first publish.
  const pending = [];
  for (const name of names) {
    const file = path.resolve(directory, `${name}-${version}.tgz`);
    const integrity = `sha512-${createHash('sha512').update(readFileSync(file)).digest('base64')}`;
    const response = await request(`https://registry.npmjs.org/${name}/${version}`);
    if (response.status === 404) {
      pending.push(file);
    } else {
      if (!response.ok) throw new Error(`Cannot check ${name}@${version}: HTTP ${response.status}`);
      const published = await response.json();
      if (published.name !== name || published.version !== version || published.dist?.integrity !== integrity) {
        throw new Error(`${name}@${version} already exists with different contents; refusing to continue`);
      }
      console.log(`Already published: ${name}@${version} (integrity verified)`);
    }
  }
  // Absolute paths cannot be misinterpreted as npm's GitHub repo shorthand.
  for (const file of pending) {
    run('npm', ['publish', file, '--access', 'public', '--provenance'], { stdio: 'inherit' });
  }
}

module.exports = { publishPackages };
if (require.main === module) {
  const [version, directory] = process.argv.slice(2);
  if (!version || !directory) throw new Error('Usage: node npm/publish.cjs <version> <artifact-directory>');
  publishPackages(version, directory).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
