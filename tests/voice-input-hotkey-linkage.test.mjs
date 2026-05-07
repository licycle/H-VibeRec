import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(
  new URL('../src-tauri/src/voice_input/platform_hotkey.rs', import.meta.url),
  'utf8',
);

assert.match(source, /fn InstallEventHandler\(/);
assert.match(source, /InstallEventHandler\(\s*GetApplicationEventTarget\(\),/s);
assert.doesNotMatch(source, /InstallApplicationEventHandler/);
