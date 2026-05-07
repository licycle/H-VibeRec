import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/lib/workspace.ts', import.meta.url), 'utf8');
const types = readFileSync(new URL('../src/types.ts', import.meta.url), 'utf8');

assert.match(source, /function assertAssistantScope\(scope: AssistantScope\)/);
assert.match(source, /scope !== 'current' && scope !== 'global'/);
assert.match(source, /throw new Error\('AI问答范围无效'\)/);
assert.match(source, /function assertWorkspaceExists\(workspaceId: string\): LocalWorkspace/);
assert.match(source, /throw new Error\('目标本地空间不存在'\)/);
assert.match(source, /if \(!question\) \{\s*throw new Error\('请输入问题'\)/s);
assert.match(source, /ask_local_notes_agent_stream/);
assert.match(source, /listenAssistantStream/);
assert.match(source, /promptTemplateId: params\.promptTemplateId \|\| null/);
assert.match(source, /webEnabled: params\.webEnabled === true/);
assert.match(source, /maxTurns: params\.maxTurns \?\? null/);
assert.match(source, /reuseUserMessageId: params\.reuseUserMessageId \|\| null/);
assert.match(source, /open_external_url/);
assert.match(source, /list_assistant_prompt_templates/);
assert.match(source, /export async function deleteAssistantSession/);
assert.match(source, /delete_assistant_session/);
assert.match(source, /export async function deleteAssistantMessagesAfter/);
assert.match(source, /delete_assistant_messages_after/);
assert.match(source, /export async function updateAssistantUserMessageAndTruncate/);
assert.match(source, /update_assistant_user_message_and_truncate/);
assert.match(source, /export async function getAssistantWorkspaceActivity/);
assert.match(source, /get_assistant_workspace_activity/);
assert.match(source, /Promise<AssistantWorkspaceActivity>/);

assert.match(types, /export interface AssistantRun/);
assert.match(types, /request_id: string/);
assert.match(types, /partial_answer: string/);
assert.match(types, /export interface AssistantWorkspaceActivity/);
assert.match(types, /active_run\?: AssistantRun \| null/);
assert.match(types, /latest_session_id\?: string \| null/);
assert.match(types, /event: 'started' \| 'delta' \| 'tool' \| 'turn' \| 'done' \| 'error'/);
assert.match(types, /run\?: AssistantRun \| null/);
