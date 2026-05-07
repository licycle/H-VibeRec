#!/usr/bin/env node
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { chmod, copyFile, cp, lstat, mkdir, mkdtemp, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import path from 'node:path';
import process from 'node:process';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const runtimeRoot = path.join(repoRoot, 'runtime', 'asr');
const sidecarRequirements = path.join(repoRoot, 'sidecars', 'funasr_nano_mlx', 'requirements.txt');
const assistantRequirements = path.join(repoRoot, 'sidecars', 'local_notes_agent', 'requirements.txt');
const pythonVersionPrefix = process.env.VOICE_VIBE_PYTHON_VERSION || '3.11';
const args = new Set(process.argv.slice(2));
const checkOnly = args.has('--check');
const repair = args.has('--repair');

main().catch(error => {
  console.error(`[asr-runtime] ${error.message}`);
  process.exit(1);
});

async function main() {
  const platform = runtimePlatform();
  if (!platform.supported) {
    throw new Error(`Bundled FunASR workflow runtime does not support ${process.platform}/${process.arch}.`);
  }

  if (existsSync(platform.pythonMarkerPath)) {
    await normalizeRuntimeEntrypoints(platform);
    if (await verifyRuntime(false)) {
      console.log(`[asr-runtime] ready: ${runtimeRoot}`);
      return;
    }
    if (!repair) {
      throw new Error(`bundled ASR runtime is incomplete; run npm run runtime:ensure -- --repair to rebuild: ${runtimeRoot}`);
    }
  }

  const ready = await verifyRuntime(false);
  if (ready) {
    console.log(`[asr-runtime] ready: ${runtimeRoot}`);
    return;
  }

  if (checkOnly) {
    throw new Error(`bundled ASR runtime is missing or incomplete: ${runtimeRoot}`);
  }

  if (existsSync(runtimeRoot) && repair) {
    await rm(runtimeRoot, { recursive: true, force: true });
  }

  await prepareRuntime();
  if (!(await verifyRuntime(true))) {
    throw new Error('bundled ASR runtime verification failed after installation');
  }
  console.log(`[asr-runtime] ready: ${runtimeRoot}`);
}

async function prepareRuntime() {
  const platform = runtimePlatform();
  await mkdir(path.dirname(runtimeRoot), { recursive: true });
  if (existsSync(runtimeRoot)) {
    throw new Error(`bundled ASR runtime already exists but is not ready; run npm run runtime:ensure -- --repair to rebuild: ${runtimeRoot}`);
  }

  const tmp = await mkdtemp(path.join(tmpdir(), 'voice-vibe-asr-runtime-'));
  try {
    const asset = await resolvePythonStandaloneAsset();
    const archive = path.join(tmp, asset.name);
    console.log(`[asr-runtime] downloading ${asset.name}`);
    await download(asset.browser_download_url, archive);

    const extracted = path.join(tmp, 'extract');
    await mkdir(extracted, { recursive: true });
    await extractArchive(archive, extracted);

    const pythonRoot = await findPythonRoot(extracted, platform);
    await cp(pythonRoot, runtimeRoot, { recursive: true });
    await normalizeRuntimeEntrypoints(platform);

    const python = pythonPath();
    await run(python, ['-m', 'ensurepip', '--upgrade'], repoRoot);
    await run(python, ['-m', 'pip', 'install', '--upgrade', 'pip'], repoRoot);
    await installSidecarRequirements();
    await normalizeRuntimeEntrypoints(platform);
  } finally {
    await rm(tmp, { recursive: true, force: true });
  }
}

async function verifyRuntime(verbose) {
  const python = pythonPath();
  const ffmpeg = ffmpegPath();
  if (!existsSync(python) || !existsSync(ffmpeg)) {
    if (verbose) console.error('[asr-runtime] missing python or ffmpeg wrapper');
    return false;
  }

  const importCheck = await runMaybe(
    python,
    [
      '-c',
      'import importlib.metadata as m; import agents, openai, huggingface_hub, imageio_ffmpeg, modelscope, socksio, numpy, torch, torchaudio, ddgs; m.version("funasr"); print("python-runtime-ok")',
    ],
    repoRoot,
  );
  if (!importCheck.ok && !checkOnly) {
    await installSidecarRequirements();
    const repairedImportCheck = await runMaybe(
      python,
      [
        '-c',
        'import importlib.metadata as m; import agents, openai, huggingface_hub, imageio_ffmpeg, modelscope, socksio, numpy, torch, torchaudio, ddgs; m.version("funasr"); print("python-runtime-ok")',
      ],
      repoRoot,
    );
    if (!repairedImportCheck.ok) {
      if (verbose) console.error(repairedImportCheck.output);
      return false;
    }
  } else if (!importCheck.ok) {
    if (verbose) console.error(importCheck.output);
    return false;
  }

  const ffmpegCheck = await runMaybe(ffmpeg, ['-version'], repoRoot);
  if (!ffmpegCheck.ok) {
    if (verbose) console.error(ffmpegCheck.output);
    return false;
  }

  return true;
}

async function installSidecarRequirements() {
  await run(pythonPath(), ['-m', 'pip', 'install', '-r', sidecarRequirements], repoRoot);
  await run(pythonPath(), ['-m', 'pip', 'install', '-r', assistantRequirements], repoRoot);
}

async function resolvePythonStandaloneAsset() {
  const release = JSON.parse(
    await fetchText('https://api.github.com/repos/astral-sh/python-build-standalone/releases/latest'),
  );
  const { pythonBuildStandaloneTarget: target } = runtimePlatform();
  const asset = release.assets?.find(item =>
    item.name.startsWith(`cpython-${pythonVersionPrefix}.`) &&
    item.name.includes(`-${target}-install_only.tar.gz`) &&
    !item.name.includes('debug'),
  );
  if (!asset) {
    throw new Error(`cannot find python-build-standalone ${pythonVersionPrefix} asset for ${target}`);
  }
  return asset;
}

function fetchText(url) {
  return runCapture('curl', ['-fL', '-sS', '-H', 'User-Agent: hit-vvc-runtime', url], repoRoot);
}

function download(url, targetPath) {
  return run(
    'curl',
    ['-fL', '-#', '-H', 'User-Agent: hit-vvc-runtime', '-o', targetPath, url],
    repoRoot,
  );
}

async function extractArchive(archive, targetDir) {
  await run('tar', ['-xzf', archive, '-C', targetDir], repoRoot);
}

async function findPythonRoot(root, platform) {
  const queue = [root];
  while (queue.length > 0) {
    const current = queue.shift();
    const marker = path.join(current, ...platform.pythonRootMarker);
    if (existsSync(marker)) return current;
    const entries = await readDirSafe(current);
    for (const entry of entries) {
      if (entry.isDirectory()) queue.push(path.join(current, entry.name));
    }
  }
  throw new Error(`extracted python archive did not contain ${platform.pythonRootMarker.join('/')}`);
}

async function readDirSafe(dir) {
  try {
    return await readdir(dir, { withFileTypes: true });
  } catch {
    return [];
  }
}

async function normalizeRuntimeEntrypoints(platform) {
  if (platform.name === 'windows-x64') {
    await writeWindowsEntrypoints();
    return;
  }
  await normalizeRuntimeSymlinks();
  await writePosixFfmpegWrapper();
}

async function normalizeRuntimeSymlinks() {
  const binDir = path.join(runtimeRoot, 'bin');
  const python311 = path.join(binDir, 'python3.11');
  if (!existsSync(python311)) {
    throw new Error(`bundled Python executable missing: ${python311}`);
  }

  await replaceSymlink(path.join(binDir, 'python'), 'python3.11');
  await replaceSymlink(path.join(binDir, 'python3'), 'python3.11');
  await replaceSymlink(path.join(binDir, '2to3'), '2to3-3.11');
  await replaceSymlink(path.join(binDir, 'idle3'), 'idle3.11');
  await replaceSymlink(path.join(binDir, 'pydoc3'), 'pydoc3.11');
  await replaceSymlink(path.join(binDir, 'python3-config'), 'python3.11-config');
  await replaceSymlink(
    path.join(runtimeRoot, 'lib', 'pkgconfig', 'python3.pc'),
    'python-3.11.pc',
  );
  await replaceSymlink(
    path.join(runtimeRoot, 'lib', 'pkgconfig', 'python3-embed.pc'),
    'python-3.11-embed.pc',
  );
  await replaceSymlink(
    path.join(runtimeRoot, 'share', 'man', 'man1', 'python3.1'),
    'python3.11.1',
  );

  await chmod(python311, 0o755);
}

async function writePosixFfmpegWrapper() {
  const wrapper = `#!/bin/sh
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$SCRIPT_DIR/python" -c 'import imageio_ffmpeg, os, sys; exe=imageio_ffmpeg.get_ffmpeg_exe(); os.execv(exe, [exe] + sys.argv[1:])' "$@"
`;
  await writeFile(ffmpegPath(), wrapper);
  await chmod(ffmpegPath(), 0o755);
}

async function writeWindowsEntrypoints() {
  const scriptsDir = path.join(runtimeRoot, 'Scripts');
  const pythonExe = path.join(runtimeRoot, 'python.exe');
  if (!existsSync(pythonExe)) {
    throw new Error(`bundled Python executable missing: ${pythonExe}`);
  }
  await mkdir(path.join(runtimeRoot, 'bin'), { recursive: true });
  await writeFile(
    path.join(runtimeRoot, 'bin', 'python.cmd'),
    '@echo off\r\n"%~dp0..\\python.exe" %*\r\n',
  );
  if (await pythonCanImportImageioFfmpeg()) {
    const source = (await runCapture(pythonExe, [
      '-c',
      'import imageio_ffmpeg; print(imageio_ffmpeg.get_ffmpeg_exe())',
    ], repoRoot)).trim();
    if (source && existsSync(source)) {
      await copyFile(source, ffmpegPath());
    }
  }
  if (existsSync(scriptsDir)) {
    await writeFile(
      path.join(runtimeRoot, 'bin', 'activate-runtime.cmd'),
      '@echo off\r\nset "PATH=%~dp0;%~dp0..\\Scripts;%PATH%"\r\n',
    );
  }
}

function pythonPath() {
  return process.platform === 'win32'
    ? path.join(runtimeRoot, 'python.exe')
    : path.join(runtimeRoot, 'bin', 'python');
}

function ffmpegPath() {
  return process.platform === 'win32'
    ? path.join(runtimeRoot, 'bin', 'ffmpeg.exe')
    : path.join(runtimeRoot, 'bin', 'ffmpeg');
}

async function pythonCanImportImageioFfmpeg() {
  const result = await runMaybe(
    pythonPath(),
    ['-c', 'import imageio_ffmpeg'],
    repoRoot,
  );
  return result.ok;
}

function runtimePlatform() {
  if (process.platform === 'darwin' && process.arch === 'arm64') {
    return {
      supported: true,
      name: 'macos-arm64',
      pythonBuildStandaloneTarget: 'aarch64-apple-darwin',
      pythonRootMarker: ['bin', 'python3'],
      pythonMarkerPath: path.join(runtimeRoot, 'bin', 'python3.11'),
    };
  }
  if (process.platform === 'win32' && process.arch === 'x64') {
    return {
      supported: true,
      name: 'windows-x64',
      pythonBuildStandaloneTarget: 'x86_64-pc-windows-msvc',
      pythonRootMarker: ['python.exe'],
      pythonMarkerPath: path.join(runtimeRoot, 'python.exe'),
    };
  }
  return {
    supported: false,
    name: `${process.platform}-${process.arch}`,
    pythonBuildStandaloneTarget: '',
    pythonRootMarker: [],
    pythonMarkerPath: '',
  };
}

function run(command, args, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, stdio: 'inherit' });
    child.on('error', reject);
    child.on('exit', code => {
      if (code === 0) resolve();
      else reject(new Error(`${command} ${args.join(' ')} exited with ${code}`));
    });
  });
}

function runMaybe(command, args, cwd) {
  return new Promise(resolve => {
    const child = spawn(command, args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '';
    child.stdout.on('data', chunk => {
      output += chunk;
    });
    child.stderr.on('data', chunk => {
      output += chunk;
    });
    child.on('error', error => resolve({ ok: false, output: error.message }));
    child.on('exit', code => resolve({ ok: code === 0, output }));
  });
}

function runCapture(command, args, cwd) {
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
      if (code === 0) resolve(stdout);
      else reject(new Error(`${command} ${args.join(' ')} exited with ${code}\n${stderr}`));
    });
  });
}

async function removePathIfPresent(targetPath) {
  try {
    await lstat(targetPath);
  } catch {
    return;
  }
  await rm(targetPath, { force: true });
}

async function replaceSymlink(targetPath, linkTarget) {
  await removePathIfPresent(targetPath);
  await symlink(linkTarget, targetPath);
}
