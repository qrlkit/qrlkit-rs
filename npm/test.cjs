// Install the real launcher tarball with local overrides for its platform dependencies.
// This exercises npm's platform selection offline, without publishing test packages.
const assert = require('node:assert/strict');
const { execFileSync, spawnSync } = require('node:child_process');
const { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { packageNpm } = require('./package.cjs');
const platforms = require('./platforms.cjs');

const binary = path.resolve(process.argv[2] || 'target/debug/qrlkit');
const temporary = realpathSync(mkdtempSync(path.join(os.tmpdir(), 'qrlkit-npm-')));
const env = { ...process.env, npm_config_cache: path.join(temporary, 'cache') };
try {
  const platform = `${process.platform}-${process.arch}`;
  const packages = packageNpm(platforms[platform], binary, temporary);
  const directories = [packages.main];
  for (const [key, target] of Object.entries(platforms)) {
    const staged = packageNpm(target, binary, path.join(temporary, key));
    const manifest = JSON.parse(readFileSync(path.join(staged.native, 'package.json')));
    assert.equal(`${manifest.os[0]}-${manifest.cpu[0]}`, key);
    const main = JSON.parse(readFileSync(path.join(staged.main, 'package.json')));
    assert.equal(main.optionalDependencies[manifest.name], staged.version);
    assert.equal(Object.keys(main.optionalDependencies).length, 4);
    directories.push(staged.native);
  }
  const tarballs = new Map();
  for (const directory of directories) {
    const packed = JSON.parse(execFileSync('npm', ['pack', directory, '--json', '--pack-destination', temporary], { env }))[0];
    assert(packed.files.some((file) => file.path === 'LICENSE'));
    assert(packed.files.some((file) => file.path.startsWith('bin/') && (file.mode & 0o111)));
    tarballs.set(packed.name, path.join(temporary, packed.filename));
  }
  const prefix = path.join(temporary, 'installed');
  mkdirSync(prefix);
  // Keep the packed launcher unchanged. Only the test project's dependency resolution
  // replaces registry downloads with our local tarballs; none is installed explicitly.
  const overrides = Object.fromEntries(Object.keys(platforms).map((key) => {
    const name = `qrlkit-${key}`;
    return [name, `file:${tarballs.get(name)}`];
  }));
  writeFileSync(path.join(prefix, 'package.json'), JSON.stringify({ private: true, overrides }));
  execFileSync('npm', ['install', '--offline', '--ignore-scripts', '--no-audit', '--no-fund',
    '--include=optional', tarballs.get('qrlkit')], { cwd: prefix, env, stdio: 'pipe', timeout: 30000 });
  const command = path.join(prefix, 'node_modules/.bin/qrlkit');
  const installed = path.join(prefix, 'node_modules/qrlkit');
  const native = path.dirname(require.resolve(`qrlkit-${platform}/package.json`, { paths: [installed] }));
  assert(native.startsWith(`${prefix}${path.sep}`), 'Platform dependency must be installed inside the test fixture');
  for (const key of Object.keys(platforms)) {
    if (key === platform) continue;
    assert.throws(() => require.resolve(`qrlkit-${key}/package.json`, { paths: [installed] }), { code: 'MODULE_NOT_FOUND' });
  }
  assert.equal(execFileSync(command, ['--version'], { encoding: 'utf8' }).trim(), `qrlkit ${packages.version}`);
  assert.match(execFileSync(command, ['--help'], { encoding: 'utf8' }), /Usage:/);
  const invalid = spawnSync(command, ['--not-a-qrlkit-option'], { encoding: 'utf8' });
  assert.equal(invalid.status, 2);
  assert.match(invalid.stderr, /--not-a-qrlkit-option/);
  const launcher = path.join(installed, 'bin/qrlkit.cjs');
  const unsupported = spawnSync(process.execPath, ['-e',
    "Object.defineProperty(process, 'arch', { value: 'unsupported' }); require(process.argv[1]);",
    launcher,
  ], { encoding: 'utf8' });
  assert.equal(unsupported.status, 1);
  assert.match(unsupported.stderr, /unsupported platform/);
  execFileSync('python3', [path.join(__dirname, '../scripts/test-terminal.py'), command], { stdio: 'inherit' });
  // A skipped/removed optional dependency should give actionable output.
  rmSync(native, { recursive: true });
  const missing = spawnSync(command, ['--version'], { encoding: 'utf8' });
  assert.equal(missing.status, 1);
  assert.match(missing.stderr, /--include=optional/);
  console.log(`npm automatic platform selection and terminal checks passed for ${platform}`);
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
