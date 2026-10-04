// Stage npm packages; Cargo.toml is the source of truth for release versions.
const { execFileSync } = require('node:child_process');
const { chmodSync, copyFileSync, mkdirSync, writeFileSync } = require('node:fs');
const path = require('node:path');
const platforms = require('./platforms.cjs');
const template = require('./package.json');
const root = path.resolve(__dirname, '..');

function packageNpm(target, binary, output) {
  const platform = Object.keys(platforms).find((key) => platforms[key] === target);
  if (!platform) throw new Error(`Unsupported Rust target: ${target}`);
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--locked', '--no-deps', '--format-version', '1'], { cwd: root }));
  const { version } = metadata.packages.find((pkg) => pkg.name === 'qrlkit');
  const [os, cpu] = platform.split('-');
  const main = path.join(output, 'qrlkit');
  const native = path.join(output, `qrlkit-${platform}`);
  for (const directory of [main, native]) {
    mkdirSync(path.join(directory, 'bin'), { recursive: true });
    copyFileSync(path.join(root, 'LICENSE'), path.join(directory, 'LICENSE'));
  }
  copyFileSync(binary, path.join(native, 'bin/qrlkit'));
  chmodSync(path.join(native, 'bin/qrlkit'), 0o755);
  copyFileSync(path.join(root, 'npm/bin/qrlkit.cjs'), path.join(main, 'bin/qrlkit.cjs'));
  chmodSync(path.join(main, 'bin/qrlkit.cjs'), 0o755);
  copyFileSync(path.join(root, 'npm/platforms.cjs'), path.join(main, 'platforms.cjs'));
  const optionalDependencies = Object.fromEntries(Object.keys(platforms).map((key) => [`qrlkit-${key}`, version]));
  writeFileSync(path.join(main, 'package.json'), JSON.stringify({ ...template, version, optionalDependencies }, null, 2) + '\n');
  writeFileSync(path.join(native, 'package.json'), JSON.stringify({
    name: `qrlkit-${platform}`, version,
    description: `qrlkit native binary for ${platform}`,
    license: template.license, repository: template.repository,
    os: [os], cpu: [cpu], files: ['bin/qrlkit'],
  }, null, 2) + '\n');
  return { main, native, version };
}

module.exports = { packageNpm };
if (require.main === module) {
  const [target, binary, output] = process.argv.slice(2);
  if (!target || !binary || !output) throw new Error('Usage: node npm/package.cjs <rust-target> <binary> <output-directory>');
  packageNpm(target, path.resolve(binary), path.resolve(output));
}
