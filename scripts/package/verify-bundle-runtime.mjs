#!/usr/bin/env node
import { existsSync } from 'node:fs';
import { readdir, stat } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const bundleRoot = path.join(repoRoot, 'src-tauri', 'target', 'release', 'bundle');

main().catch(error => {
  console.error(`[bundle-runtime] ${error.message}`);
  process.exit(1);
});

async function main() {
  const bundle = await findBundleResources();
  const resourcesDir = bundle.resourcesDir;
  const runtimeRoot = path.join(resourcesDir, 'runtime', 'asr');
  const pythonPath = runtimePythonPath(runtimeRoot);
  const ffmpegPath = runtimeFfmpegPath(runtimeRoot);
  const sidecarPath = path.join(resourcesDir, 'sidecars', 'funasr_nano_mlx', 'main.py');
  const assistantSidecarPath = path.join(resourcesDir, 'sidecars', 'local_notes_agent', 'main.py');
  const assistantToolsPath = path.join(
    resourcesDir,
    'sidecars',
    'local_notes_agent',
    'notes_mcp_server.py',
  );
  const assistantWebToolsPath = path.join(
    resourcesDir,
    'sidecars',
    'local_notes_agent',
    'web_mcp_server.py',
  );
  const requirementsPath = path.join(
    resourcesDir,
    'sidecars',
    'funasr_nano_mlx',
    'requirements.txt',
  );
  const assistantRequirementsPath = path.join(
    resourcesDir,
    'sidecars',
    'local_notes_agent',
    'requirements.txt',
  );

  const requiredPaths = [
    pythonPath,
    ffmpegPath,
    sidecarPath,
    assistantSidecarPath,
    assistantToolsPath,
    assistantWebToolsPath,
    requirementsPath,
    assistantRequirementsPath,
    runtimePythonLibPath(runtimeRoot),
  ];
  for (const requiredPath of requiredPaths) {
    if (!existsSync(requiredPath)) {
      throw new Error(`missing bundled runtime file: ${path.relative(bundle.root, requiredPath)}`);
    }
  }

  if (process.platform === 'darwin') {
    await run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', bundle.root], repoRoot);
  }

  const staleQwenSidecar = path.join(resourcesDir, 'sidecars', 'qwen3asr');
  if (existsSync(staleQwenSidecar)) {
    throw new Error(`stale qwen3asr sidecar was bundled: ${staleQwenSidecar}`);
  }
  const staleQwenEntry = path.join(runtimeRoot, 'bin', 'mlx-qwen3-asr');
  if (existsSync(staleQwenEntry)) {
    throw new Error(`stale mlx-qwen3-asr entrypoint was bundled: ${staleQwenEntry}`);
  }

  await run(ffmpegPath, ['-version'], repoRoot);
  await run(
    pythonPath,
    [
      '-c',
      [
        'import importlib.metadata as m',
        'import agents, openai, funasr, huggingface_hub, imageio_ffmpeg, modelscope, numpy, torch, torchaudio, ddgs',
        'm.version("funasr")',
        'm.version("imageio-ffmpeg")',
        'm.version("ddgs")',
        'print("bundle-runtime-ok")',
      ].join('; '),
    ],
    repoRoot,
  );

  console.log(`[bundle-runtime] verified: ${bundle.root}`);
}

async function findBundleResources() {
  if (process.platform === 'darwin') {
    const appBundle = await findMacAppBundle();
    return {
      root: appBundle,
      resourcesDir: path.join(appBundle, 'Contents', 'Resources'),
    };
  }
  if (process.platform === 'win32') {
    return findWindowsBundleResources();
  }
  throw new Error(`bundle runtime verification does not support ${process.platform}`);
}

async function findMacAppBundle() {
  const macosBundleDir = path.join(bundleRoot, 'macos');
  const entries = await readdir(macosBundleDir).catch(() => []);
  const apps = [];
  for (const entry of entries) {
    if (!entry.endsWith('.app')) continue;
    const fullPath = path.join(macosBundleDir, entry);
    const info = await stat(fullPath).catch(() => null);
    if (info?.isDirectory()) apps.push(fullPath);
  }
  if (apps.length === 0) {
    throw new Error(`no macOS .app bundle found under ${macosBundleDir}`);
  }
  apps.sort();
  return apps[0];
}

async function findWindowsBundleResources() {
  const candidates = [
    path.join(bundleRoot, 'nsis'),
    path.join(bundleRoot, 'msi'),
    path.join(repoRoot, 'src-tauri', 'target', 'release'),
  ];
  for (const candidate of candidates) {
    const resourceDirs = await findResourceDirs(candidate);
    const match = resourceDirs.find(dir =>
      existsSync(path.join(dir, 'runtime', 'asr')) &&
      existsSync(path.join(dir, 'sidecars', 'funasr_nano_mlx', 'main.py')),
    );
    if (match) {
      return {
        root: path.dirname(match),
        resourcesDir: match,
      };
    }
  }
  throw new Error('no Windows bundle resource directory containing runtime/asr was found');
}

async function findResourceDirs(root) {
  const results = [];
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop();
    const entries = await readdir(current, { withFileTypes: true }).catch(() => []);
    for (const entry of entries) {
      const fullPath = path.join(current, entry.name);
      if (!entry.isDirectory()) continue;
      if (entry.name === 'Resources' || entry.name === 'resources') {
        results.push(fullPath);
      }
      stack.push(fullPath);
    }
  }
  return results;
}

function runtimePythonPath(runtimeRoot) {
  return process.platform === 'win32'
    ? path.join(runtimeRoot, 'python.exe')
    : path.join(runtimeRoot, 'bin', 'python');
}

function runtimeFfmpegPath(runtimeRoot) {
  return process.platform === 'win32'
    ? path.join(runtimeRoot, 'bin', 'ffmpeg.exe')
    : path.join(runtimeRoot, 'bin', 'ffmpeg');
}

function runtimePythonLibPath(runtimeRoot) {
  return process.platform === 'win32'
    ? path.join(runtimeRoot, 'Lib')
    : path.join(runtimeRoot, 'lib', 'python3.11');
}

function run(command, args, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', chunk => {
      stdout += chunk;
    });
    child.stderr.on('data', chunk => {
      stderr += chunk;
    });
    child.on('error', reject);
    child.on('exit', code => {
      if (code === 0) {
        if (stdout.trim()) console.log(stdout.trim());
        resolve();
      } else {
        reject(new Error(`${command} ${args.join(' ')} exited with ${code}\n${stderr}${stdout}`));
      }
    });
  });
}
