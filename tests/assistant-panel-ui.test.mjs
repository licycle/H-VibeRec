import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const panelSource = readFileSync(new URL('../src/components/AssistantPanel.tsx', import.meta.url), 'utf8');
const panelCss = readFileSync(new URL('../src/components/AssistantPanel.css', import.meta.url), 'utf8');

assert.match(panelSource, /ASSISTANT_PANEL_OPTIONS_PREFIX/);
assert.match(panelSource, /toolbarCollapsed/);
assert.match(panelSource, /maxTurns/);
assert.match(panelSource, /setMaxTurns/);
assert.match(panelSource, /usedTurns/);
assert.match(panelSource, /setUsedTurns/);
assert.match(panelSource, /status: 'local'/);
assert.match(panelSource, /function isLocalUserMessage/);
assert.match(panelSource, /isLocalUserMessage\(message\)/);
assert.match(panelSource, /role="switch"/);
assert.match(panelSource, /assistant-max-turns/);
assert.match(panelSource, /assistant-turn-usage/);
assert.match(panelSource, /已用/);
assert.match(panelSource, /deleteAssistantSession/);
assert.match(panelSource, /deleteAssistantMessagesAfter/);
assert.match(panelSource, /updateAssistantUserMessageAndTruncate/);
assert.match(panelSource, /reuseUserMessageId/);
assert.match(panelSource, /confirmLocalAction/);
assert.match(panelSource, /getAssistantWorkspaceActivity/);
assert.match(panelSource, /activity\.active_run/);
assert.match(panelSource, /activeRequestIdRef\.current = activity\.active_run\.request_id/);
assert.match(panelSource, /event\.event === 'started'/);
assert.match(panelSource, /event\.event === 'done'/);
assert.match(panelSource, /dedupeAssistantMessages/);
assert.match(panelSource, /runToPendingMessages/);
assert.match(panelSource, /function resolveAssistantPromptTemplateId/);
assert.match(panelSource, /promptTemplates\.some\(template => template\.id === value\)/);
assert.match(panelSource, /const effectivePromptTemplateId = resolveAssistantPromptTemplateId/);
assert.match(panelSource, /promptTemplateId: effectivePromptTemplateId \|\| null/);
assert.match(panelSource, /const activeWorkspaceIdRef = useRef/);
assert.match(panelSource, /const activeRequestWorkspaceIdRef = useRef/);
assert.match(panelSource, /activeWorkspaceIdRef\.current = workspaceId/);
assert.match(panelSource, /activeRequestWorkspaceIdRef\.current = requestWorkspaceId/);
assert.match(panelSource, /isActiveAssistantRequest\(requestId, requestWorkspaceId\)/);
assert.match(panelSource, /if \(!isActiveAssistantRequest\(requestId, requestWorkspaceId\)\) return/);
assert.match(panelSource, /const activeRequestWorkspaceId = activeRequestWorkspaceIdRef\.current/);
assert.match(panelSource, /!isActiveAssistantRequest\(event\.request_id, activeRequestWorkspaceId\)/);
assert.match(panelSource, /cancelled \|\| !isActiveWorkspace\(workspaceId\)/);
assert.match(panelSource, /activeRequestWorkspaceIdRef\.current = null/);

const editedSubmitSource = panelSource.slice(
  panelSource.indexOf('const submitEditedUserMessage = useCallback'),
  panelSource.indexOf('  return (', panelSource.indexOf('const submitEditedUserMessage = useCallback'))
);
assert.match(editedSubmitSource, /const requestWorkspaceId = workspaceId/);
assert.match(editedSubmitSource, /promptTemplateId: effectivePromptTemplateId \|\| null/);
assert.match(editedSubmitSource, /if \(!isActiveAssistantRequest\(requestId, requestWorkspaceId\)\) return/);
assert.match(editedSubmitSource, /isActiveAssistantRequest\(requestId, requestWorkspaceId\)/);
assert.match(panelCss, /\.assistant-toolbar\.collapsed/);
assert.match(panelCss, /\.assistant-switch/);
