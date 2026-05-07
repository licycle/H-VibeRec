import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(
  new URL('../scripts/package/check-package-readiness.mjs', import.meta.url),
  'utf8',
);

for (const requiredPath of [
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
]) {
  assert.match(source, new RegExp(escapeRegExp(requiredPath)));
}

for (const requiredPath of [
  'src-tauri/src/voice_input/mod.rs',
  'src-tauri/src/voice_input/hotkey.rs',
  'src-tauri/src/voice_input/insertion.rs',
  'src-tauri/src/voice_input/recorder.rs',
  'src-tauri/src/voice_input/text.rs',
]) {
  assert.match(source, new RegExp(escapeRegExp(requiredPath)));
}

for (const commandName of [
  'start_voice_input_dictation',
  'stop_voice_input_dictation',
  'cancel_voice_input_dictation',
  'toggle_voice_input_dictation',
  'get_voice_input_status',
  'get_voice_input_stats',
  'check_voice_input_permissions',
  'request_voice_input_accessibility_permission',
]) {
  assert.match(source, new RegExp(escapeRegExp(commandName)));
}

assert.match(source, /checkVoiceInputInsertionRuntimeContract/);
assert.match(source, /pbcopy/);
assert.match(source, /pbpaste/);
assert.match(source, /NSPasteboard/);
assert.match(source, /K_CG_HID_EVENT_TAP/);
assert.match(source, /K_CG_SESSION_EVENT_TAP/);
assert.match(source, /CGEventSetFlags\(up, 0\)/);

assert.match(source, /checkDmgInstallExperience/);
assert.match(source, /\.\.\/assets\/branding\/generated\/dmg-background\.png/);
assert.match(source, /applicationFolderPosition/);
assert.match(source, /appPosition/);

assert.match(source, /import agents, openai, funasr, huggingface_hub, imageio_ffmpeg, modelscope, socksio, numpy, torch, torchaudio, ddgs/);
assert.match(source, /m\.version\("ddgs"\)/);

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
