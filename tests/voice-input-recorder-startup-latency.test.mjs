import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const recorderSource = readFileSync(new URL('../src-tauri/src/voice_input/recorder.rs', import.meta.url), 'utf8');
const audioCoreSource = readFileSync(new URL('../src-tauri/src/audio/core.rs', import.meta.url), 'utf8');

const activeRecorderStartSource = sliceBetween(
  recorderSource,
  'pub async fn start() -> Result<Self, String>',
  'pub async fn stop(self) -> Result<Vec<f32>, String>',
);
assert.doesNotMatch(
  activeRecorderStartSource,
  /trigger_audio_permission/,
  'voice input hot path should not run the 1s microphone permission warmup stream',
);

const audioStreamFromDeviceSource = sliceBetween(
  audioCoreSource,
  'pub async fn from_device',
  'pub async fn subscribe',
);
assert.match(
  audioStreamFromDeviceSource,
  /ready_rx\.await/,
  'AudioStream::from_device should wait for the worker thread to start the stream',
);
assert.ok(
  audioStreamFromDeviceSource.indexOf('stream.play()') < audioStreamFromDeviceSource.indexOf('ready_rx.await'),
  'AudioStream::from_device should only resolve after stream.play() has been attempted',
);

function sliceBetween(text, startNeedle, endNeedle) {
  const start = text.indexOf(startNeedle);
  const end = text.indexOf(endNeedle);
  assert.notEqual(start, -1, `Missing start marker: ${startNeedle}`);
  assert.notEqual(end, -1, `Missing end marker: ${endNeedle}`);
  assert.ok(start < end, `Expected ${startNeedle} before ${endNeedle}`);
  return text.slice(start, end);
}
