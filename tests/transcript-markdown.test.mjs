import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/lib/transcriptMarkdown.ts', import.meta.url), 'utf8');
const executableSource = source
  .replace('export function transcriptToMarkdown', 'function transcriptToMarkdown')
  .replace('title: string', 'title')
  .replace('text: string', 'text')
  .replace('): string {', ') {');
const factory = new Function(
  `${executableSource}
return { transcriptToMarkdown };`
);
const { transcriptToMarkdown } = factory();

const transcript = [
  '[50ms - 410ms] Speaker 0: 对的，',
  '[410ms - 2910ms] Speaker 0: 因为这个也是我们擅长的，',
  '[36230ms - 41030ms] Speaker 1: 所以就是因为深圳深源拓科技，',
].join('\n');

const markdown = transcriptToMarkdown('转录 - demo', transcript);

assert.equal(
  markdown,
  [
    '# 转录 - demo',
    '',
    '[50ms - 410ms] Speaker 0: 对的，',
    '',
    '[410ms - 2910ms] Speaker 0: 因为这个也是我们擅长的，',
    '',
    '[36230ms - 41030ms] Speaker 1: 所以就是因为深圳深源拓科技，',
  ].join('\n')
);
assert.ok(!markdown.includes('```'));
assert.ok(markdown.includes('[50ms - 410ms] Speaker 0: 对的，\n\n[410ms - 2910ms]'));
