import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(
  new URL('../src-tauri/src/voice_input/insertion.rs', import.meta.url),
  'utf8',
);

assert.doesNotMatch(source, /pbcopy|pbpaste/);
assert.doesNotMatch(source, /Command::new|Stdio::piped|std::process::Command|std::io::Write/);
assert.match(source, /NSPasteboard/);
assert.match(source, /generalPasteboard/);
assert.match(source, /public\.utf8-plain-text/);
assert.match(source, /stringForType:/);
assert.match(source, /setString:forType:/);

assert.doesNotMatch(source, /K_CG_HID_EVENT_TAP/);
assert.match(source, /const\s+K_CG_SESSION_EVENT_TAP:\s*u32\s*=\s*1;/);
assert.match(source, /CGEventPost\(K_CG_SESSION_EVENT_TAP,\s*down\)/);
assert.match(source, /CGEventSetFlags\(up,\s*0\)/);
assert.match(source, /CGEventPost\(K_CG_SESSION_EVENT_TAP,\s*up\)/);
