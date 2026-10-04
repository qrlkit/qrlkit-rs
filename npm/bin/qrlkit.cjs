#!/usr/bin/env node
const { spawn } = require('node:child_process');
const { optionalDependencies } = require('../package.json');

const platform = `${process.platform}-${process.arch}`;
const packageName = `qrlkit-${platform}`;
if (!Object.hasOwn(optionalDependencies, packageName)) {
  const supported = Object.keys(optionalDependencies).map((name) => name.slice('qrlkit-'.length));
  console.error(`qrlkit: unsupported platform ${platform}. Supported: ${supported.join(', ')}.`);
  process.exit(1);
}

let binary;
try {
  binary = require.resolve(`${packageName}/bin/qrlkit`);
} catch {
  console.error(`qrlkit: missing ${packageName}. Reinstall qrlkit with optional dependencies enabled (npm install -g qrlkit --include=optional).`);
  process.exit(1);
}

const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit' });
const handlers = new Map();
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  const handler = () => child.kill(signal);
  handlers.set(signal, handler);
  process.on(signal, handler);
}
child.on('error', (error) => {
  console.error(`qrlkit: could not start ${packageName}: ${error.message}`);
  process.exitCode = 1;
});
child.on('exit', (code, signal) => {
  for (const [name, handler] of handlers) process.removeListener(name, handler);
  if (signal) process.kill(process.pid, signal);
  else process.exitCode = code;
});
