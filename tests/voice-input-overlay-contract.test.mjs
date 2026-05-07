import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const tauriConfig = JSON.parse(readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const appSource = readFileSync(new URL('../src/App.tsx', import.meta.url), 'utf8');
const overlaySource = readFileSync(new URL('../src/components/VoiceInputOverlay.tsx', import.meta.url), 'utf8');
const overlayStyleSource = readFileSync(new URL('../src/components/VoiceInputOverlay.css', import.meta.url), 'utf8');
const servicesSource = readFileSync(new URL('../src/services/index.ts', import.meta.url), 'utf8');
const commandsSource = readFileSync(new URL('../src-tauri/src/commands/voice_input.rs', import.meta.url), 'utf8');
const libSource = readFileSync(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8');
const voiceInputRustSource = readFileSync(new URL('../src-tauri/src/voice_input/mod.rs', import.meta.url), 'utf8');
const voiceInputOverlayRustSource = readFileSync(new URL('../src-tauri/src/voice_input/overlay.rs', import.meta.url), 'utf8');
const voiceInputPlatformHotkeySource = readFileSync(new URL('../src-tauri/src/voice_input/platform_hotkey.rs', import.meta.url), 'utf8');

const overlayWindow = tauriConfig.app.windows.find(window => window.label === 'voice-input-overlay');
assert.ok(overlayWindow, 'Tauri config should declare the voice-input-overlay window');
assert.equal(overlayWindow.visible, false);
assert.equal(overlayWindow.transparent, true);
assert.equal(overlayWindow.decorations, false);
assert.equal(overlayWindow.resizable, false);
assert.equal(overlayWindow.width, 256);
assert.equal(overlayWindow.height, 32);
assert.equal(overlayWindow.backgroundColor, '#00000000');
assert.equal(overlayWindow.alwaysOnTop, true);
assert.equal(overlayWindow.skipTaskbar, true);
assert.equal(overlayWindow.visibleOnAllWorkspaces, true);
assert.equal(Object.hasOwn(overlayWindow, 'focusable'), false);
assert.match(voiceInputOverlayRustSource, /set_focusable\(false\)/);
assert.match(voiceInputOverlayRustSource, /const VOICE_INPUT_OVERLAY_WIDTH: f64 = 256\.0;/);
assert.match(voiceInputOverlayRustSource, /const VOICE_INPUT_OVERLAY_HEIGHT: f64 = 32\.0;/);
assert.match(voiceInputOverlayRustSource, /set_size\(tauri::LogicalSize::new/);
assert.doesNotMatch(
  sliceBetween(voiceInputOverlayRustSource, 'fn show_voice_input_overlay', 'fn hide_voice_input_overlay'),
  /PhysicalSize/,
);

const mainCapability = tauriConfig.app.security.capabilities.find(capability => capability.identifier === 'main');
assert.ok(mainCapability.windows.includes('voice-input-overlay'));
assert.ok(mainCapability.permissions.includes('core:window:allow-start-dragging'));
assert.ok(!mainCapability.permissions.includes('core:window:allow-outer-position'));
assert.ok(!mainCapability.permissions.includes('core:window:allow-set-position'));

assert.match(appSource, /getCurrentWindow/);
assert.match(appSource, /voice-input-overlay/);
assert.match(appSource, /<VoiceInputOverlay\s+standalone\s*\/>/);
assert.doesNotMatch(sliceBetween(appSource, 'function AppContent()', 'function App()'), /<VoiceInputOverlay/);
assert.match(appSource, /document\.documentElement\.classList\.toggle\('voice-input-overlay-window', isOverlayWindow\)/);
assert.match(libSource, /WindowEvent::CloseRequested/);
assert.match(libSource, /window\.label\(\) == "main"/);
assert.match(libSource, /api\.prevent_close\(\)/);
assert.match(libSource, /window\.hide\(\)/);
assert.match(overlayStyleSource, /html\.voice-input-overlay-window,\s*\nbody\.voice-input-overlay-window/);
assert.match(overlayStyleSource, /body\.voice-input-overlay-window #root/);
assert.match(overlayStyleSource, /background:\s*transparent/);
assert.match(overlayStyleSource, /background-color:\s*rgba\(0,\s*0,\s*0,\s*0\.7\)/);
assert.match(overlayStyleSource, /width:\s*100%/);
assert.match(overlayStyleSource, /height:\s*100%/);
assert.match(sliceCssBlock(overlayStyleSource, '.voice-input-overlay-pill'), /justify-content:\s*center/);
assert.match(sliceCssBlock(overlayStyleSource, '.voice-input-overlay-pill'), /border:\s*none/);
assert.match(sliceCssBlock(overlayStyleSource, '.voice-input-overlay-pill'), /border-radius:\s*999px/);
assert.match(sliceCssBlock(overlayStyleSource, '.voice-input-overlay-pill'), /box-shadow:\s*none/);
assert.doesNotMatch(overlayStyleSource, /border-color:/);
assert.doesNotMatch(sliceCssBlock(overlayStyleSource, '.voice-input-overlay-pill'), /backdrop-filter/);
assert.match(sliceCssBlock(overlayStyleSource, '.voice-input-overlay-open-target'), /justify-content:\s*center/);
assert.match(sliceCssBlock(overlayStyleSource, '.voice-input-overlay-open-target'), /text-align:\s*center/);
assert.doesNotMatch(sliceCssBlock(overlayStyleSource, '.voice-input-overlay-open-target'), /text-align:\s*left/);
assert.match(overlayStyleSource, /\.voice-input-overlay-open-target/);
assert.match(overlayStyleSource, /\.voice-input-overlay-open-target:hover[\s\S]*background:\s*transparent/);
assert.match(overlayStyleSource, /\.voice-input-overlay-drag-handle/);
assert.match(overlayStyleSource, /\.voice-input-overlay-action:hover:not\(:disabled\)[\s\S]*background:\s*transparent/);
assert.match(overlayStyleSource, /\.voice-input-overlay-drag-handle:hover[\s\S]*background:\s*transparent/);

const showOverlaySource = sliceBetween(voiceInputOverlayRustSource, 'fn show_voice_input_overlay', 'fn hide_voice_input_overlay');
assert.doesNotMatch(showOverlaySource, /position_voice_input_overlay/);
const startDictationSource = sliceBetween(voiceInputRustSource, 'pub async fn start_dictation', 'pub async fn stop_dictation');
const startingStatusIndex = startDictationSource.indexOf('emit_status(&app, "starting"');
const recorderStartIndex = startDictationSource.indexOf('recorder::ActiveShortRecorder::start().await');
const listeningStatusIndex = startDictationSource.indexOf('emit_status(&app, "listening"');
const enterRegistrationIndex = startDictationSource.indexOf('platform_hotkey::register_enter_submit()');
assert.notEqual(startingStatusIndex, -1, 'start_dictation should emit starting before opening the recorder');
assert.notEqual(recorderStartIndex, -1, 'start_dictation should open the short recorder');
assert.notEqual(listeningStatusIndex, -1, 'start_dictation should emit listening after the recorder opens');
assert.notEqual(enterRegistrationIndex, -1, 'start_dictation should register Enter submit');
assert.ok(
  startingStatusIndex < recorderStartIndex,
  'starting status should be emitted before the microphone stream is opened',
);
assert.ok(
  recorderStartIndex < listeningStatusIndex,
  'listening status should wait until the microphone stream is opened',
);
assert.ok(
  listeningStatusIndex < enterRegistrationIndex,
  'listening status should not wait for Enter submit registration',
);
assert.match(voiceInputRustSource, /VoiceInputPhase::Starting => "starting"/);
assert.match(voiceInputRustSource, /VoiceInputPhase::Starting => "麦克风启动中，请稍候"/);
assert.doesNotMatch(voiceInputRustSource, /VoiceInputPhase::Starting => "正在启动听写"/);
assert.match(voiceInputRustSource, /EscapePressed/);
assert.match(voiceInputRustSource, /notify_escape_pressed/);
assert.match(voiceInputRustSource, /is_escape_key_code\(key_code: i64\)[\s\S]*matches!\(key_code,\s*53\)/);
assert.match(voiceInputRustSource, /HotkeyCommand::EscapePressed[\s\S]*cancel_dictation\(app_for_task\.clone\(\)\)/);
assert.match(voiceInputRustSource, /Enter 完成 · Esc 取消/);
assert.doesNotMatch(voiceInputRustSource, /Enter 完成 · Esc 取消 · \{hotkey_label\} 结束/);
assert.doesNotMatch(voiceInputRustSource, /正在听写 · \{hotkey_label\} 结束/);
assert.match(voiceInputPlatformHotkeySource, /is_escape_key_code\(key_code\)/);
assert.match(voiceInputPlatformHotkeySource, /notify_escape_pressed\(\)/);
assert.match(sliceBetween(voiceInputOverlayRustSource, 'fn is_overlay_visible_phase', 'fn is_overlay_final_phase'), /"starting"/);
assert.match(
  sliceBetween(voiceInputRustSource, 'pub async fn toggle_dictation', 'async fn process_samples'),
  /VoiceInputPhase::Starting[\s\S]*Ok\(status\(\)\)/,
);

assert.doesNotMatch(overlaySource, /Window\.getByLabel\('main'\)/);
assert.match(overlaySource, /'starting'/);
assert.match(overlaySource, /case 'starting':\s*return '麦克风启动中，请稍候'/);
assert.doesNotMatch(overlaySource, /case 'starting':\s*return '正在启动听写'/);
assert.match(overlaySource, /openMainWindow\(\)/);
assert.match(commandsSource, /open_main_window_from_voice_input_overlay/);
assert.match(libSource, /open_main_window_from_voice_input_overlay/);
assert.match(overlaySource, /startDragging\(\)/);
assert.match(overlaySource, /aria-label="拖动控件"/);
assert.match(overlaySource, /voice-input-overlay-drag-handle/);
assert.match(overlaySource, /voice-input-overlay-open-target/);
assert.match(overlaySource, /onPointerDown=\{event => void handleDragStart\(event\)\}/);
assert.match(overlaySource, /onClick=\{event => void handleOpenMain\(event\)\}/);
assert.doesNotMatch(overlaySource, /className=\{`voice-input-overlay-pill \$\{event\.phase\}`\}[\s\S]{0,160}onClick=\{event => void handleOpenMain\(event\)\}/);
assert.doesNotMatch(overlaySource, /outerPosition\(\)/);
assert.doesNotMatch(overlaySource, /setPosition\(new PhysicalPosition/);
assert.doesNotMatch(overlaySource, /setPointerCapture/);
assert.doesNotMatch(overlaySource, /aria-label="打开主控件"/);
assert.doesNotMatch(overlaySource, /voice-input-overlay-open-main/);
assert.match(overlaySource, /event\.stopPropagation\(\)/);
assert.match(overlaySource, /cancelDictation/);
assert.match(overlaySource, /stopDictation/);
assert.match(overlaySource, /aria-label="取消语音输入"/);
assert.match(overlaySource, /aria-label="确认语音输入"/);
assert.match(overlaySource, /cancelled/);

assert.match(servicesSource, /cancelDictation\(\): Promise<VoiceInputStatus>/);
assert.match(servicesSource, /invoke\('cancel_voice_input_dictation'\)/);
assert.match(commandsSource, /cancel_voice_input_dictation/);
assert.match(libSource, /cancel_voice_input_dictation/);

function sliceBetween(text, startNeedle, endNeedle) {
  const start = text.indexOf(startNeedle);
  const end = text.indexOf(endNeedle);
  assert.notEqual(start, -1, `Missing start marker: ${startNeedle}`);
  assert.notEqual(end, -1, `Missing end marker: ${endNeedle}`);
  assert.ok(start < end, `Expected ${startNeedle} before ${endNeedle}`);
  return text.slice(start, end);
}

function sliceCssBlock(text, selector) {
  const start = text.indexOf(`${selector} {`);
  assert.notEqual(start, -1, `Missing CSS selector: ${selector}`);
  const end = text.indexOf('\n}', start);
  assert.notEqual(end, -1, `Missing CSS block close for: ${selector}`);
  return text.slice(start, end);
}
