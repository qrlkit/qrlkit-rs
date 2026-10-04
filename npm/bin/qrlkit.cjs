#!/usr/bin/env node
const { spawn } = require('node:child_process');
const platforms = require('../platforms.cjs');

const platform = `${process.platform}-${process.arch}`;
if (!platforms[platform]) {
  console.error(`qrlkit: unsupported platform ${platform}. Supported: ${Object.keys(platforms).join(', ')}.`);
  process.exit(1);
}

const packageName = `qrlkit-${platform}`;
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
