#!/usr/bin/env node
import { spawn } from 'node:child_process';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
const command = args[0] || '';

main().catch(error => {
  console.error(error.message);
  process.exit(1);
});

async function main() {
  if (command === 'dev' || command === 'build') {
    await run(process.execPath, [path.join(repoRoot, 'scripts', 'runtime', 'ensure-asr-runtime.mjs')], repoRoot);
  }

  const tauriBin = process.platform === 'win32'
    ? path.join(repoRoot, 'node_modules', '.bin', 'tauri.cmd')
    : path.join(repoRoot, 'node_modules', '.bin', 'tauri');
  await run(tauriBin, args, repoRoot);

  if (command === 'build') {
    await run(process.execPath, [path.join(repoRoot, 'scripts', 'package', 'verify-bundle-runtime.mjs')], repoRoot);
  }
}

function run(commandPath, commandArgs, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(commandPath, commandArgs, { cwd, stdio: 'inherit' });
    child.on('error', reject);
    child.on('exit', code => {
      if (code === 0) resolve();
      else reject(new Error(`${commandPath} ${commandArgs.join(' ')} exited with ${code}`));
    });
  });
}
