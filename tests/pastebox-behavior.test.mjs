import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

// Run real, pure UI decision functions instead of checking source spelling.
const source = readFileSync(new URL('../src/lib/pastebox.ts', import.meta.url), 'utf8');
const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext } }).outputText;
const { resolvePasteTarget, mergePasteItems, pasteStatus } = await import(`data:text/javascript;base64,${Buffer.from(js).toString('base64')}`);

const current = { id: 'current', available: true };
const original = { id: 'original-id', available: true };
const state = { targets: [current], opening_target_id: current.id };
const a = { id: 'a', seq: 1, target: original, version: 0, delivery_status: 'pending', processing_status: 'ready' };
assert.equal(resolvePasteTarget(a, state, 'opening'), current.id);
assert.equal(resolvePasteTarget(a, state, 'original'), original.id);
assert.equal(resolvePasteTarget(a, state, current.id), current.id);
assert.throws(() => resolvePasteTarget(a, { ...state, opening_target_id: null }, 'opening'), /没有可用的位置/);
assert.throws(() => resolvePasteTarget({ ...a, target: { ...original, available: false } }, state, 'original'), /原位置不可用/);
assert.throws(() => resolvePasteTarget(a, state, 'missing'), /没有可用的位置/);
assert.throws(() => resolvePasteTarget(a, { ...state, targets: [{ ...current, available: false }] }, 'opening'));

const b = { ...a, id: 'b', seq: 2 };
const newA = { ...a, version: 1, delivery_status: 'copied' };
assert.deepEqual(mergePasteItems([a, b], [newA], true), [b, newA]);
assert.deepEqual(mergePasteItems([a, b], [newA], false), [newA]);
assert.equal(pasteStatus({ ...a, processing_status: 'refining' }), '润色中 · 原文可用');
assert.equal(pasteStatus({ ...newA, processing_status: 'refining' }), '已复制');
assert.equal(pasteStatus({ ...a, delivery_status: 'uncertain' }), '请检查上次粘贴');
assert.notEqual(pasteStatus({ ...a, delivery_status: 'sent' }), pasteStatus({ ...a, delivery_status: 'verified' }));
assert.equal(pasteStatus({ ...a, processing_status: 'queued' }), '等待转录');
assert.equal(pasteStatus({ ...a, processing_status: 'transcribing' }), '转录中');
assert.equal(pasteStatus({ ...a, processing_status: 'failed' }), '转录未完成 · 可重试');
assert.equal(pasteStatus({ ...a, mode: 'automatic' }), '等待自动回写');
