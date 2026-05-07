#!/usr/bin/env node
import { existsSync } from 'node:fs';
import { readFile, readdir } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const errors = [];
const voiceInputCommands = [
  'start_voice_input_dictation',
  'stop_voice_input_dictation',
  'cancel_voice_input_dictation',
  'toggle_voice_input_dictation',
  'get_voice_input_status',
  'get_voice_input_stats',
  'check_voice_input_permissions',
  'request_voice_input_accessibility_permission',
];

main().catch(error => {
  console.error(`[package-check] ${error.message}`);
  process.exit(1);
});

async function main() {
  await checkPlatform();
  await checkPackageScripts();
  await checkTauriResources();
  await checkDmgInstallExperience();
  await checkSourceLayout();
  await checkVoiceInputPackaging();
  await checkVoiceInputInsertionRuntimeContract();
  await checkBundledRuntime();
  await checkPackagingScript();

  if (errors.length > 0) {
    for (const error of errors) {
      console.error(`[package-check] FAIL: ${error}`);
    }
    process.exit(1);
  }

  console.log('[package-check] ready: package config and bundled runtime are installable after model download');
}

async function checkPlatform() {
  if (!isSupportedRuntimePlatform()) {
    fail('offline ASR packaging currently supports macOS Apple Silicon and Windows x64 only');
  }
}

async function checkPackageScripts() {
  const packageJson = await readJson('package.json');
  const scripts = packageJson.scripts || {};
  expectScriptIncludes(scripts, 'runtime:check', 'scripts/runtime/ensure-asr-runtime.mjs --check');
  expectScriptIncludes(scripts, 'bundle:verify-runtime', 'scripts/package/verify-bundle-runtime.mjs');
  expectScriptIncludes(scripts, 'package:check', 'scripts/package/check-package-readiness.mjs');
  expectScriptIncludes(scripts, 'tauri', 'scripts/runtime/tauri-runtime-wrapper.mjs');
}

async function checkTauriResources() {
  const config = await readJson('src-tauri/tauri.conf.json');
  if (process.platform === 'darwin') {
    const signingIdentity = config?.bundle?.macOS?.signingIdentity;
    if (typeof signingIdentity !== 'string' || signingIdentity.trim().length === 0) {
      fail('src-tauri/tauri.conf.json bundle.macOS.signingIdentity must be set for installable macOS builds; use "-" for ad-hoc local packages or a Developer ID identity for release');
    }
  }

  const resources = config?.bundle?.resources;
  if (!resources || Array.isArray(resources) || typeof resources !== 'object') {
    fail('src-tauri/tauri.conf.json bundle.resources must be an explicit source-to-target map');
    return;
  }

  expectResource(resources, '../runtime', 'runtime');
  expectResource(
    resources,
    '../sidecars/funasr_nano_mlx/main.py',
    'sidecars/funasr_nano_mlx/main.py',
  );
  expectResource(
    resources,
    '../sidecars/funasr_nano_mlx/requirements.txt',
    'sidecars/funasr_nano_mlx/requirements.txt',
  );
  expectResource(
    resources,
    '../sidecars/local_notes_agent/main.py',
    'sidecars/local_notes_agent/main.py',
  );
  expectResource(
    resources,
    '../sidecars/local_notes_agent/notes_mcp_server.py',
    'sidecars/local_notes_agent/notes_mcp_server.py',
  );
  expectResource(
    resources,
    '../sidecars/local_notes_agent/web_mcp_server.py',
    'sidecars/local_notes_agent/web_mcp_server.py',
  );
  expectResource(
    resources,
    '../sidecars/local_notes_agent/requirements.txt',
    'sidecars/local_notes_agent/requirements.txt',
  );
  if (Object.keys(resources).some(key => key === '../sidecars' || key.includes('qwen3asr'))) {
    fail('bundle.resources must not include the whole sidecars directory or qwen3asr');
  }
}

async function checkSourceLayout() {
  const requiredPaths = [
    'scripts/dev/start.sh',
    'scripts/dev/start.bat',
    'scripts/setup/install-dependencies.sh',
    'scripts/setup/install-dependencies.bat',
    'scripts/package/package-app.sh',
    'scripts/package/package-app.bat',
    'scripts/package/check-package-readiness.mjs',
    'scripts/package/verify-bundle-runtime.mjs',
    'scripts/runtime/ensure-asr-runtime.mjs',
    'scripts/runtime/tauri-runtime-wrapper.mjs',
    'scripts/assets/generate-dmg-background.swift',
    'scripts/assets/generate-rounded-icons.swift',
    'scripts/debug/debug-windows.bat',
    'assets/branding/source/icon-1024.png',
    'assets/branding/generated/icon-1024-rounded.png',
    'assets/branding/generated/dmg-background.png',
    'sidecars/funasr_nano_mlx/main.py',
    'sidecars/funasr_nano_mlx/requirements.txt',
    'sidecars/local_notes_agent/main.py',
    'sidecars/local_notes_agent/notes_mcp_server.py',
    'sidecars/local_notes_agent/web_mcp_server.py',
    'sidecars/local_notes_agent/requirements.txt',
    'src-tauri/src/commands/voice_input.rs',
    'src-tauri/src/voice_input/mod.rs',
    'src-tauri/src/voice_input/hotkey.rs',
    'src-tauri/src/voice_input/insertion.rs',
    'src-tauri/src/voice_input/recorder.rs',
    'src-tauri/src/voice_input/text.rs',
  ];
  requiredPaths.push(...runtimeRequiredPaths());
  for (const requiredPath of requiredPaths) {
    if (!existsSync(path.join(repoRoot, requiredPath))) {
      fail(`missing required package path: ${requiredPath}`);
    }
  }

  const removedRootPaths = [
    'dev-start.sh',
    'dev-start.bat',
    'install-dependencies.sh',
    'install-dependencies.bat',
    'package-app.sh',
    'package-app.bat',
    'debug-windows.bat',
    'scripts/check-package-readiness.mjs',
    'scripts/ensure-asr-runtime.mjs',
    'scripts/verify-bundle-runtime.mjs',
    'scripts/tauri-runtime-wrapper.mjs',
    'scripts/generate-rounded-icons.swift',
    'icon-1024.png',
    'icon-1024-rounded.png',
    'plan.md',
    'IMPLEMENTATION_SUMMARY.md',
  ];
  for (const removedPath of removedRootPaths) {
    if (existsSync(path.join(repoRoot, removedPath))) {
      fail(`legacy root path must be removed: ${removedPath}`);
    }
  }

  if (existsSync(path.join(repoRoot, 'sidecars', 'qwen3asr'))) {
    fail('stale sidecars/qwen3asr directory must be removed');
  }

  const sidecarRust = await readText('src-tauri/src/sidecar.rs');
  if (!sidecarRust.includes('funasr_nano_mlx')) {
    fail('src-tauri/src/sidecar.rs must resolve the FunASR workflow sidecar');
  }
  if (sidecarRust.toLowerCase().includes('qwen3asr')) {
    fail('src-tauri/src/sidecar.rs still references qwen3asr');
  }
}

async function checkDmgInstallExperience() {
  if (process.platform !== 'darwin') return;

  const config = await readJson('src-tauri/tauri.conf.json');
  const dmg = config?.bundle?.macOS?.dmg;
  const expectedBackground = '../assets/branding/generated/dmg-background.png';

  if (!dmg || Array.isArray(dmg) || typeof dmg !== 'object') {
    fail('src-tauri/tauri.conf.json bundle.macOS.dmg must configure the macOS drag-to-Applications install experience');
    return;
  }

  if (dmg.background !== expectedBackground) {
    fail(`src-tauri/tauri.conf.json bundle.macOS.dmg.background must be ${expectedBackground}`);
  }

  expectDmgSize(dmg.windowSize, 660, 400, 'windowSize');
  expectDmgPosition(dmg.appPosition, 180, 190, 'appPosition');
  expectDmgPosition(dmg.applicationFolderPosition, 480, 190, 'applicationFolderPosition');

  const backgroundPath = path.resolve(repoRoot, 'src-tauri', dmg.background || '');
  if (!existsSync(backgroundPath)) {
    fail(`missing macOS DMG background image: ${expectedBackground}`);
    return;
  }

  const imageSize = await readPngDimensions(backgroundPath).catch(error => {
    fail(`invalid macOS DMG background image ${expectedBackground}: ${error.message}`);
    return null;
  });
  if (imageSize && (imageSize.width !== 660 || imageSize.height !== 400)) {
    fail(`macOS DMG background image must be 660x400; found ${imageSize.width}x${imageSize.height}`);
  }
}

async function checkVoiceInputPackaging() {
  const libRust = await readText('src-tauri/src/lib.rs');
  for (const needle of [
    'mod voice_input;',
    'crate::voice_input::init(app.handle().clone())',
    ...voiceInputCommands,
  ]) {
    if (!libRust.includes(needle)) {
      fail(`src-tauri/src/lib.rs must register voice input packaging contract: ${needle}`);
    }
  }

  const commandsModRust = await readText('src-tauri/src/commands/mod.rs');
  for (const needle of [
    'pub mod voice_input;',
    ...voiceInputCommands,
  ]) {
    if (!commandsModRust.includes(needle)) {
      fail(`src-tauri/src/commands/mod.rs must export voice input command: ${needle}`);
    }
  }

  const frontendServices = await readText('src/services/index.ts');
  for (const command of voiceInputCommands) {
    if (!frontendServices.includes(command)) {
      fail(`src/services/index.ts must expose voice input command invoke: ${command}`);
    }
  }
}

async function checkVoiceInputInsertionRuntimeContract() {
  const source = await readText('src-tauri/src/voice_input/insertion.rs');
  for (const forbidden of [
    'pbcopy',
    'pbpaste',
    'Command::new',
    'Stdio::piped',
    'std::process::Command',
    'std::io::Write',
    'K_CG_HID_EVENT_TAP',
  ]) {
    if (source.includes(forbidden)) {
      fail(`src-tauri/src/voice_input/insertion.rs must not use packaged-unsafe insertion path: ${forbidden}`);
    }
  }

  for (const required of [
    'NSPasteboard',
    'generalPasteboard',
    'public.utf8-plain-text',
    'stringForType:',
    'setString:forType:',
    'K_CG_SESSION_EVENT_TAP',
    'CGEventPost(K_CG_SESSION_EVENT_TAP, down)',
    'CGEventSetFlags(up, 0)',
    'CGEventPost(K_CG_SESSION_EVENT_TAP, up)',
  ]) {
    if (!source.includes(required)) {
      fail(`src-tauri/src/voice_input/insertion.rs must preserve packaged voice input insertion contract: ${required}`);
    }
  }

  if (!/const\s+K_CG_SESSION_EVENT_TAP:\s*u32\s*=\s*1;/.test(source)) {
    fail('src-tauri/src/voice_input/insertion.rs must post paste events through kCGSessionEventTap (1)');
  }
}

async function checkBundledRuntime() {
  const pythonPath = path.join(repoRoot, ...runtimePythonPath());
  const ffmpegPath = path.join(repoRoot, ...runtimeFfmpegPath());
  if (!existsSync(pythonPath) || !existsSync(ffmpegPath)) return;

  await run(ffmpegPath, ['-version']);
  await run(pythonPath, [
    '-c',
    [
      'import importlib.metadata as m',
      'import agents, openai, funasr, huggingface_hub, imageio_ffmpeg, modelscope, socksio, numpy, torch, torchaudio, ddgs',
      'm.version("funasr")',
      'm.version("imageio-ffmpeg")',
      'm.version("ddgs")',
      'print("runtime-import-ok")',
    ].join('; '),
  ]);

  const qwenShow = await runMaybe(pythonPath, ['-m', 'pip', 'show', 'mlx-qwen3-asr']);
  if (qwenShow.ok) {
    fail('runtime/asr still has stale Python package mlx-qwen3-asr installed');
  }
  if (existsSync(path.join(repoRoot, 'runtime', 'asr', 'bin', 'mlx-qwen3-asr'))) {
    fail('runtime/asr still has stale mlx-qwen3-asr executable');
  }
  await failIfDirectoryEntryIncludes(
    path.join(repoRoot, 'runtime', 'asr', 'lib', 'python3.11', 'site-packages'),
    'mlx_qwen3_asr',
    'runtime/asr still contains stale mlx_qwen3_asr site-package files',
  );
}

async function checkPackagingScript() {
  await checkShellPackagingScript();
  await checkWindowsPackagingScript();

  const wrapper = await readText('scripts/runtime/tauri-runtime-wrapper.mjs');
  if (!wrapper.includes('verify-bundle-runtime.mjs')) {
    fail('scripts/runtime/tauri-runtime-wrapper.mjs must verify bundle runtime after build');
  }
}

async function checkShellPackagingScript() {
  const script = await readText('scripts/package/package-app.sh');
  for (const needle of [
    'uname -s',
    'uname -m',
    'npm run runtime:check',
    'npm run package:check',
    'rm -rf src-tauri/target/release/bundle src-tauri/target/release/_up_',
    'npm run tauri build',
  ]) {
    if (!script.includes(needle)) {
      fail(`scripts/package/package-app.sh must include: ${needle}`);
    }
  }
}

async function checkWindowsPackagingScript() {
  const script = await readText('scripts/package/package-app.bat');
  for (const needle of [
    'PROCESSOR_ARCHITECTURE',
    'npm run runtime:check',
    'npm run package:check',
    'npm run tauri build',
  ]) {
    if (!script.includes(needle)) {
      fail(`scripts/package/package-app.bat must include: ${needle}`);
    }
  }
  if (script.includes('Windows packaging is disabled')) {
    fail('scripts/package/package-app.bat still disables Windows packaging');
  }
}

function expectScriptIncludes(scripts, name, needle) {
  const value = scripts[name];
  if (typeof value !== 'string' || !value.includes(needle)) {
    fail(`package.json script "${name}" must include "${needle}"`);
  }
}

function expectResource(resources, source, target) {
  if (resources[source] !== target) {
    fail(`bundle resource ${source} must map to ${target}`);
  }
}

function expectDmgPosition(position, x, y, name) {
  if (!position || position.x !== x || position.y !== y) {
    fail(`src-tauri/tauri.conf.json bundle.macOS.dmg.${name} must be { "x": ${x}, "y": ${y} }`);
  }
}

function expectDmgSize(size, width, height, name) {
  if (!size || size.width !== width || size.height !== height) {
    fail(`src-tauri/tauri.conf.json bundle.macOS.dmg.${name} must be { "width": ${width}, "height": ${height} }`);
  }
}

async function failIfDirectoryEntryIncludes(dir, needle, message) {
  const entries = await readdir(dir).catch(() => []);
  if (entries.some(entry => entry.toLowerCase().includes(needle.toLowerCase()))) {
    fail(message);
  }
}

async function readJson(relativePath) {
  return JSON.parse(await readText(relativePath));
}

async function readText(relativePath) {
  return readFile(path.join(repoRoot, relativePath), 'utf8');
}

async function readPngDimensions(absolutePath) {
  const buffer = await readFile(absolutePath);
  const signature = buffer.subarray(0, 8).toString('hex');
  if (signature !== '89504e470d0a1a0a') {
    throw new Error('not a PNG file');
  }
  if (buffer.length < 24) {
    throw new Error('PNG header is incomplete');
  }
  return {
    width: buffer.readUInt32BE(16),
    height: buffer.readUInt32BE(20),
  };
}

function fail(message) {
  errors.push(message);
}

function isSupportedRuntimePlatform() {
  return (process.platform === 'darwin' && process.arch === 'arm64') ||
    (process.platform === 'win32' && process.arch === 'x64');
}

function runtimeRequiredPaths() {
  if (process.platform === 'win32') {
    return [
      'runtime/asr/python.exe',
      'runtime/asr/bin/ffmpeg.exe',
    ];
  }
  return [
    'runtime/asr/bin/python',
    'runtime/asr/bin/ffmpeg',
  ];
}

function runtimePythonPath() {
  return process.platform === 'win32'
    ? ['runtime', 'asr', 'python.exe']
    : ['runtime', 'asr', 'bin', 'python'];
}

function runtimeFfmpegPath() {
  return process.platform === 'win32'
    ? ['runtime', 'asr', 'bin', 'ffmpeg.exe']
    : ['runtime', 'asr', 'bin', 'ffmpeg'];
}

function run(command, args) {
  return runMaybe(command, args).then(result => {
    if (!result.ok) {
      fail(`${command} ${args.join(' ')} failed: ${result.output.trim()}`);
    }
  });
}

function runMaybe(command, args) {
  return new Promise(resolve => {
    const child = spawn(command, args, { cwd: repoRoot, stdio: ['ignore', 'pipe', 'pipe'] });
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
