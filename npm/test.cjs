// Exercise real tarballs offline, including platform selection and the installed command.
const assert = require('node:assert/strict');
const { execFileSync, spawnSync } = require('node:child_process');
const { mkdtempSync, readFileSync, rmSync } = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { packageNpm } = require('./package.cjs');
const platforms = require('./platforms.cjs');

const binary = path.resolve(process.argv[2] || 'target/debug/qrlkit');
const temporary = mkdtempSync(path.join(os.tmpdir(), 'qrlkit-npm-'));
const env = { ...process.env, npm_config_cache: path.join(temporary, 'cache') };
try {
  const platform = `${process.platform}-${process.arch}`;
  const packages = packageNpm(platforms[platform], binary, temporary);
  for (const [key, target] of Object.entries(platforms)) {
    const staged = packageNpm(target, binary, path.join(temporary, key));
    const manifest = JSON.parse(readFileSync(path.join(staged.native, 'package.json')));
    assert.equal(`${manifest.os[0]}-${manifest.cpu[0]}`, key);
    const main = JSON.parse(readFileSync(path.join(staged.main, 'package.json')));
    assert.equal(main.optionalDependencies[manifest.name], staged.version);
    assert.equal(Object.keys(main.optionalDependencies).length, 4);
  }
  const tarballs = [packages.main, packages.native].map((directory) => {
    const packed = JSON.parse(execFileSync('npm', ['pack', directory, '--json', '--pack-destination', temporary], { env }))[0];
    assert(packed.files.some((file) => file.path === 'LICENSE'));
    assert(packed.files.some((file) => file.path.startsWith('bin/') && (file.mode & 0o111)));
    return path.join(temporary, packed.filename);
  });
  const prefix = path.join(temporary, 'installed');
  execFileSync('npm', ['install', '--global', '--prefix', prefix, '--offline', '--ignore-scripts', '--no-audit', '--no-fund', '--include=optional', ...tarballs], { env, stdio: 'pipe' });
  const command = path.join(prefix, 'bin/qrlkit');
  assert.equal(execFileSync(command, ['--version'], { encoding: 'utf8' }).trim(), `qrlkit ${packages.version}`);
  assert.match(execFileSync(command, ['--help'], { encoding: 'utf8' }), /Usage:/);
  const invalid = spawnSync(command, ['--not-a-qrlkit-option'], { encoding: 'utf8' });
  assert.equal(invalid.status, 2);
  assert.match(invalid.stderr, /--not-a-qrlkit-option/);
  const launcher = path.join(prefix, 'lib/node_modules/qrlkit/bin/qrlkit.cjs');
  const unsupported = spawnSync(process.execPath, ['-e',
    "Object.defineProperty(process, 'arch', { value: 'unsupported' }); require(process.argv[1]);",
    launcher,
  ], { encoding: 'utf8' });
  assert.equal(unsupported.status, 1);
  assert.match(unsupported.stderr, /unsupported platform/);
  execFileSync('python3', [path.join(__dirname, '../scripts/test-terminal.py'), command], { stdio: 'inherit' });
  // A skipped/removed optional dependency should give actionable output.
  rmSync(path.join(prefix, 'lib/node_modules', `qrlkit-${platform}`), { recursive: true });
  const missing = spawnSync(command, ['--version'], { encoding: 'utf8' });
  assert.equal(missing.status, 1);
  assert.match(missing.stderr, /--include=optional/);
  console.log(`npm tarball and terminal checks passed for ${platform}`);
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
