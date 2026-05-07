import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/components/RecordingSettingsTab.tsx', import.meta.url), 'utf8');

assert.match(source, /function modifierOnlyHotkeyFromKeyboardEvent/);
assert.match(source, /function hasPrimaryModifier/);
assert.match(source, /请按下 Command、Option、Control，或修饰键加普通键/);
assert.match(source, /isModifierOnlyHotkey\(nextHotkey\)/);
assert.match(source, /modifierCaptureTimeoutRef/);
