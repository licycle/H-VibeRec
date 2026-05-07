import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/components/RecordingSettingsTab.tsx', import.meta.url), 'utf8');
const css = readFileSync(new URL('../src/components/RecordingSettingsTab.css', import.meta.url), 'utf8');

assert.match(source, /className="settings-top-actions"/);
assert.match(css, /\.settings-top-actions/);

const generalPane = sliceBetween(
  source,
  "{activePane === 'general' && (",
  "{activePane === 'templates' && ("
);
assertBefore(generalPane, '保存配置', '<div className="settings-grid">', 'general save action should be above settings fields');
assertBefore(generalPane, '保存配置', 'API Key 保存在系统 keychain', 'general save action should not stay in the footer');
assertBefore(generalPane, '测试 LLM', '<div className="settings-grid">', 'general test action should be above settings fields');
assertBefore(generalPane, '测试 LLM', 'API Key 保存在系统 keychain', 'general test action should not stay in the footer');

const templatesPane = sliceBetween(
  source,
  "{activePane === 'templates' && (",
  "{activePane === 'assistant' && ("
);
assertBefore(templatesPane, '新建模板', '<div className="template-settings-grid">', 'template new action should be above template editor');
assertBefore(templatesPane, '保存模板', '<div className="template-settings-grid">', 'template save action should be above template editor');
assertBefore(templatesPane, '删除模板', '<div className="template-settings-grid">', 'template delete action should be above template editor');

const assistantPane = sliceBetween(
  source,
  "{activePane === 'assistant' && (",
  "{activePane === 'voiceInput' && ("
);
assertBefore(assistantPane, '新建模板', '<div className="template-settings-grid">', 'assistant template new action should be above assistant editor');
assertBefore(assistantPane, '保存模板', '<div className="template-settings-grid">', 'assistant template save action should be above assistant editor');
assertBefore(assistantPane, '删除模板', '<div className="template-settings-grid">', 'assistant template delete action should be above assistant editor');

const voiceInputPane = sliceBetween(
  source,
  "{activePane === 'voiceInput' && (",
  "{activePane === 'diagnostics' && ("
);
assertBefore(voiceInputPane, '保存配置', '<div className="settings-grid">', 'voice input save action should be above voice input fields');
assertBefore(voiceInputPane, '保存配置', '快捷键会在保存后重新注册', 'voice input save action should not stay in the footer');
for (const label of [
  '检查权限',
  '打开麦克风权限设置',
  '请求辅助功能授权',
  '打开辅助功能权限设置',
]) {
  assertBefore(voiceInputPane, label, '<div className="settings-grid">', `voice input ${label} action should be above voice input fields`);
  assertBefore(voiceInputPane, label, '快捷键会在保存后重新注册', `voice input ${label} action should not stay in the footer`);
}

const diagnosticsPane = sliceBetween(
  source,
  "{activePane === 'diagnostics' && (",
  '</div>\n    </div>\n  );'
);
for (const label of [
  '验证录音权限',
  '打开麦克风权限设置',
  '打开系统音频权限设置',
  '检测运行环境',
  '下载/检查 FunASR workflow 模型',
  '中断下载',
  '测试 LLM',
]) {
  assertBefore(diagnosticsPane, label, '<div className="diagnostics-grid">', `diagnostics ${label} action should be above diagnostics cards`);
}

function sliceBetween(text, startNeedle, endNeedle) {
  const start = text.indexOf(startNeedle);
  const end = text.indexOf(endNeedle);
  assert.notEqual(start, -1, `Missing start marker: ${startNeedle}`);
  assert.notEqual(end, -1, `Missing end marker: ${endNeedle}`);
  assert.ok(start < end, `Expected ${startNeedle} before ${endNeedle}`);
  return text.slice(start, end);
}

function assertBefore(text, beforeNeedle, afterNeedle, message) {
  const before = text.indexOf(beforeNeedle);
  const after = text.indexOf(afterNeedle);
  assert.notEqual(before, -1, `Missing marker: ${beforeNeedle}`);
  assert.notEqual(after, -1, `Missing marker: ${afterNeedle}`);
  assert.ok(before < after, message);
}
