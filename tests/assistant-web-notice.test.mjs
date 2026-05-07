import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const appSource = readFileSync(new URL('../src/App.tsx', import.meta.url), 'utf8');
const rightSidebarSource = readFileSync(new URL('../src/components/RightSidebar.tsx', import.meta.url), 'utf8');
const assistantPanelSource = readFileSync(new URL('../src/components/AssistantPanel.tsx', import.meta.url), 'utf8');

assert.match(appSource, /联网搜索已开启/);
assert.match(appSource, /请保持当前网络环境可以访问 Google，否则搜索可能超时或无结果。/);
assert.match(appSource, /setTimeout\(\(\) => setWebSearchNotice\(false\), 10_000\)/);
assert.match(appSource, /clearTimeout\(timer\)/);
assert.match(appSource, /onWebSearchEnabled=\{handleWebSearchEnabled\}/);
assert.match(rightSidebarSource, /onWebSearchEnabled\?: \(\) => void/);
assert.match(rightSidebarSource, /onWebSearchEnabled=\{props\.onWebSearchEnabled\}/);
assert.match(assistantPanelSource, /onWebSearchEnabled\?: \(\) => void/);
assert.match(assistantPanelSource, /const toggleWebEnabled = useCallback\(\(\) => \{\s*setWebEnabled\(value => \{\s*if \(!value\) \{\s*onWebSearchEnabled\?\.\(\);\s*\}\s*return !value;\s*\}\);\s*\}, \[onWebSearchEnabled\]\);/s);
