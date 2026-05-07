/**
 * Local Workspace Management
 * Keeps local spaces separated by workspace folder id.
 */

import {
  AssistantAskResult,
  AssistantMessage,
  AssistantPromptTemplate,
  AssistantScope,
  AssistantSession,
  AssistantSource,
  AssistantStreamEvent,
  AssistantWorkspaceActivity,
  LocalWorkspace,
  NoteDoc,
} from '../types';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

const DEFAULT_WORKSPACE_ID = 'local-workspace';
const WORKSPACES_KEY = 'local-workspaces';
const ACTIVE_WORKSPACE_KEY = 'active-local-workspace-id';
const RESET_FLAG_KEY = 'local-folder-workspaces-reset-v3';

function nowISO() {
  return new Date().toISOString();
}

function slugify(value: string) {
  const normalized = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9\u4e00-\u9fa5]+/g, '-')
    .replace(/^-+|-+$/g, '');
  return normalized || 'workspace';
}

function scopedKey(workspaceId: string, suffix: string) {
  return `local-workspace:${workspaceId}:${suffix}`;
}

function defaultWorkspace(): LocalWorkspace {
  return {
    id: DEFAULT_WORKSPACE_ID,
    type: 'local',
    title: '本地工作区',
    folderName: DEFAULT_WORKSPACE_ID,
    created: nowISO(),
    updated: nowISO(),
  };
}

function resetLocalFolderWorkspaceState() {
  if (localStorage.getItem(RESET_FLAG_KEY) === 'true') return;

  Object.keys(localStorage).forEach(key => {
    if (
      key === WORKSPACES_KEY ||
      key === ACTIVE_WORKSPACE_KEY ||
      key === 'local-workspace' ||
      key === 'local-notes' ||
      key === 'local-recordings' ||
      key === 'meetings' ||
      key.startsWith('local-folder-workspaces-reset-') ||
      key.startsWith('local-workspace:') ||
      key.startsWith('meeting-notes:') ||
      key.startsWith('meeting-recordings:')
    ) {
      localStorage.removeItem(key);
    }
  });

  const workspace = defaultWorkspace();
  localStorage.setItem(WORKSPACES_KEY, JSON.stringify([workspace]));
  localStorage.setItem(ACTIVE_WORKSPACE_KEY, workspace.id);
  localStorage.setItem(RESET_FLAG_KEY, 'true');
}

function safeParse<T>(value: string | null, fallback: T): T {
  if (!value) return fallback;
  try {
    return JSON.parse(value) as T;
  } catch {
    return fallback;
  }
}

function normalizeWorkspace(value: Partial<LocalWorkspace> | null | undefined): LocalWorkspace {
  const fallback = defaultWorkspace();
  const id = value?.id?.trim() || fallback.id;
  const title = value?.title?.trim() || fallback.title;
  return {
    id,
    type: 'local',
    title,
    folderName: value?.folderName?.trim() || slugify(title) || id,
    path: value?.path,
    created: value?.created || fallback.created,
    updated: value?.updated || fallback.updated,
  };
}

function initializeFolderWorkspaceState() {
  resetLocalFolderWorkspaceState();
}

export function loadLocalWorkspaces(): LocalWorkspace[] {
  initializeFolderWorkspaceState();
  const workspaces = safeParse<LocalWorkspace[]>(localStorage.getItem(WORKSPACES_KEY), [])
    .map(normalizeWorkspace);
  if (workspaces.length > 0) return workspaces;

  const workspace = defaultWorkspace();
  saveLocalWorkspaces([workspace]);
  localStorage.setItem(ACTIVE_WORKSPACE_KEY, workspace.id);
  return [workspace];
}

export function saveLocalWorkspaces(workspaces: LocalWorkspace[]) {
  const normalized = workspaces.map(normalizeWorkspace);
  localStorage.setItem(WORKSPACES_KEY, JSON.stringify(normalized));
}

export function syncLocalWorkspacesFromFolders(folderNames: string[]): LocalWorkspace[] {
  const existing = loadLocalWorkspaces();
  const byFolder = new Map(existing.map(workspace => [workspace.folderName, workspace]));
  const diskFolders = [...new Set(folderNames.map(folder => folder.trim()).filter(Boolean))];
  const folders = diskFolders.length > 0 ? diskFolders : [DEFAULT_WORKSPACE_ID];

  const workspaces = folders.map(folderName => {
    const existingWorkspace = byFolder.get(folderName);
    if (existingWorkspace) return existingWorkspace;
    return normalizeWorkspace({
      id: folderName === DEFAULT_WORKSPACE_ID ? DEFAULT_WORKSPACE_ID : crypto.randomUUID(),
      title: folderName === DEFAULT_WORKSPACE_ID ? '本地工作区' : folderName,
      folderName,
      created: nowISO(),
      updated: nowISO(),
    });
  });

  saveLocalWorkspaces(workspaces);
  const activeId = localStorage.getItem(ACTIVE_WORKSPACE_KEY);
  if (!activeId || !workspaces.some(workspace => workspace.id === activeId)) {
    localStorage.setItem(ACTIVE_WORKSPACE_KEY, workspaces[0].id);
  }
  return workspaces;
}

export function getActiveLocalWorkspaceId(): string {
  const workspaces = loadLocalWorkspaces();
  const activeId = localStorage.getItem(ACTIVE_WORKSPACE_KEY);
  if (activeId && workspaces.some(workspace => workspace.id === activeId)) {
    return activeId;
  }
  const firstId = workspaces[0].id;
  localStorage.setItem(ACTIVE_WORKSPACE_KEY, firstId);
  return firstId;
}

export function setActiveLocalWorkspaceId(workspaceId: string) {
  localStorage.setItem(ACTIVE_WORKSPACE_KEY, workspaceId);
}

export function getLocalWorkspace(workspaceId = getActiveLocalWorkspaceId()): LocalWorkspace {
  const workspaces = loadLocalWorkspaces();
  return workspaces.find(workspace => workspace.id === workspaceId) || workspaces[0];
}

export function createLocalWorkspace(title: string): LocalWorkspace {
  const workspaces = loadLocalWorkspaces();
  const baseTitle = title.trim() || '新建工作区';
  const baseSlug = slugify(baseTitle);
  let folderName = baseSlug;
  let index = 2;
  const usedFolders = new Set(workspaces.map(workspace => workspace.folderName));
  while (usedFolders.has(folderName)) {
    folderName = `${baseSlug}-${index}`;
    index += 1;
  }

  const workspace: LocalWorkspace = {
    id: crypto.randomUUID(),
    type: 'local',
    title: baseTitle,
    folderName,
    created: nowISO(),
    updated: nowISO(),
  };
  saveLocalWorkspaces([workspace, ...workspaces]);
  setActiveLocalWorkspaceId(workspace.id);
  return workspace;
}

export function updateWorkspaceTitle(workspaceId: string, title: string): LocalWorkspace {
  let updatedWorkspace = getLocalWorkspace(workspaceId);
  const workspaces = loadLocalWorkspaces().map(workspace => {
    if (workspace.id !== workspaceId) return workspace;
    updatedWorkspace = {
      ...workspace,
      title: title.trim() || '本地工作区',
      updated: nowISO(),
    };
    return updatedWorkspace;
  });
  saveLocalWorkspaces(workspaces);
  return updatedWorkspace;
}

export function deleteLocalWorkspace(workspaceId: string) {
  const workspaces = loadLocalWorkspaces();
  if (workspaces.length <= 1) return;

  const nextWorkspaces = workspaces.filter(workspace => workspace.id !== workspaceId);
  saveLocalWorkspaces(nextWorkspaces);
  localStorage.removeItem(scopedKey(workspaceId, 'notes'));
  localStorage.removeItem(scopedKey(workspaceId, 'recordings'));

  if (getActiveLocalWorkspaceId() === workspaceId) {
    setActiveLocalWorkspaceId(nextWorkspaces[0].id);
  }
}

/**
 * Clear active local workspace data.
 */
export function clearLocalWorkspace(workspaceId = getActiveLocalWorkspaceId()) {
  localStorage.removeItem(scopedKey(workspaceId, 'notes'));
  localStorage.removeItem(scopedKey(workspaceId, 'recordings'));
}

// ============ Notes Management ============

function loadLegacyLocalNotes(workspaceId = getActiveLocalWorkspaceId()): NoteDoc[] {
  return safeParse<NoteDoc[]>(localStorage.getItem(scopedKey(workspaceId, 'notes')), []);
}

function saveLegacyLocalNotes(notes: NoteDoc[], workspaceId = getActiveLocalWorkspaceId()) {
  localStorage.setItem(scopedKey(workspaceId, 'notes'), JSON.stringify(notes));
}

function clearLegacyLocalNotes(workspaceId = getActiveLocalWorkspaceId()) {
  localStorage.removeItem(scopedKey(workspaceId, 'notes'));
}

function deleteLegacyLocalNote(noteId: string, workspaceId = getActiveLocalWorkspaceId()) {
  const notes = loadLegacyLocalNotes(workspaceId).filter(n => n.id !== noteId);
  if (notes.length > 0) {
    saveLegacyLocalNotes(notes, workspaceId);
  } else {
    clearLegacyLocalNotes(workspaceId);
  }
}

async function readWorkspaceNotesFromDisk(workspaceFolder: string): Promise<NoteDoc[]> {
  return invoke<NoteDoc[]>('list_workspace_notes', { workspaceFolder });
}

export async function listWorkspaceNotes(workspaceId = getActiveLocalWorkspaceId()): Promise<NoteDoc[]> {
  const workspace = getLocalWorkspace(workspaceId);
  const diskNotes = await readWorkspaceNotesFromDisk(workspace.folderName);

  if (diskNotes.length > 0) {
    return diskNotes;
  }

  const legacyNotes = loadLegacyLocalNotes(workspaceId);
  if (legacyNotes.length === 0) {
    return [];
  }

  for (const note of legacyNotes) {
    await saveWorkspaceNote(note, workspaceId);
  }
  clearLegacyLocalNotes(workspaceId);
  return readWorkspaceNotesFromDisk(workspace.folderName);
}

export async function saveWorkspaceNote(
  note: NoteDoc,
  workspaceId = getActiveLocalWorkspaceId()
): Promise<NoteDoc> {
  const workspace = getLocalWorkspace(workspaceId);
  const noteToSave: NoteDoc = {
    ...note,
    id: note.id || crypto.randomUUID(),
    title: note.title?.trim() || '未命名笔记',
    created: note.created || nowISO(),
    updated: note.updated || nowISO(),
  };

  return invoke<NoteDoc>('save_workspace_note', {
    workspaceFolder: workspace.folderName,
    note: noteToSave,
  });
}

export async function createWorkspaceNote(
  title?: string,
  content?: string,
  workspaceId = getActiveLocalWorkspaceId()
): Promise<NoteDoc> {
  return saveWorkspaceNote(
    {
      id: crypto.randomUUID(),
      title: title?.trim() || '未命名笔记',
      content: content || '',
      created: nowISO(),
      updated: nowISO(),
    },
    workspaceId
  );
}

export async function deleteWorkspaceNote(
  noteId: string,
  workspaceId = getActiveLocalWorkspaceId()
): Promise<void> {
  const workspace = getLocalWorkspace(workspaceId);
  await invoke('delete_workspace_note', {
    workspaceFolder: workspace.folderName,
    noteId,
  });
  deleteLegacyLocalNote(noteId, workspaceId);
}

// ============ Assistant Management ============

function assertAssistantScope(scope: AssistantScope): void {
  if (scope !== 'current' && scope !== 'global') {
    throw new Error('AI问答范围无效');
  }
}

function assertWorkspaceExists(workspaceId: string): LocalWorkspace {
  const workspace = getLocalWorkspace(workspaceId);
  if (!workspace?.folderName) {
    throw new Error('目标本地空间不存在');
  }
  return workspace;
}

export async function askLocalNotesAgent(
  params: {
    sessionId?: string | null;
    question: string;
    scope: AssistantScope;
    workspaceId?: string;
    promptTemplateId?: string | null;
    webEnabled?: boolean;
    maxTurns?: string | number | null;
    reuseUserMessageId?: string | null;
  }
): Promise<AssistantAskResult> {
  const question = params.question.trim();
  if (!question) {
    throw new Error('请输入问题');
  }
  assertAssistantScope(params.scope);
  const workspace = assertWorkspaceExists(params.workspaceId || getActiveLocalWorkspaceId());
  if (params.scope === 'current' && !workspace.folderName) {
    throw new Error('当前空间模式必须指定本地空间');
  }

  return invoke<AssistantAskResult>('ask_local_notes_agent', {
    request: {
      sessionId: params.sessionId || null,
      question,
      scope: params.scope,
      workspaceFolder: workspace.folderName,
      promptTemplateId: params.promptTemplateId || null,
      webEnabled: params.webEnabled === true,
      maxTurns: params.maxTurns ?? null,
      reuseUserMessageId: params.reuseUserMessageId || null,
    },
  });
}

export async function askLocalNotesAgentStream(
  params: {
    requestId: string;
    sessionId?: string | null;
    question: string;
    scope: AssistantScope;
    workspaceId?: string;
    promptTemplateId?: string | null;
    webEnabled?: boolean;
    maxTurns?: string | number | null;
    reuseUserMessageId?: string | null;
  }
): Promise<AssistantAskResult> {
  const requestId = params.requestId.trim();
  const question = params.question.trim();
  if (!requestId) {
    throw new Error('请求 ID 不能为空');
  }
  if (!question) {
    throw new Error('请输入问题');
  }
  assertAssistantScope(params.scope);
  const workspace = assertWorkspaceExists(params.workspaceId || getActiveLocalWorkspaceId());
  if (params.scope === 'current' && !workspace.folderName) {
    throw new Error('当前空间模式必须指定本地空间');
  }

  return invoke<AssistantAskResult>('ask_local_notes_agent_stream', {
    request: {
      requestId,
      sessionId: params.sessionId || null,
      question,
      scope: params.scope,
      workspaceFolder: workspace.folderName,
      promptTemplateId: params.promptTemplateId || null,
      webEnabled: params.webEnabled === true,
      maxTurns: params.maxTurns ?? null,
      reuseUserMessageId: params.reuseUserMessageId || null,
    },
  });
}

export async function getAssistantWorkspaceActivity(
  workspaceId = getActiveLocalWorkspaceId()
): Promise<AssistantWorkspaceActivity> {
  const workspace = assertWorkspaceExists(workspaceId);
  return invoke<AssistantWorkspaceActivity>('get_assistant_workspace_activity', {
    workspaceFolder: workspace.folderName,
  });
}

export function listenAssistantStream(
  handler: (event: AssistantStreamEvent) => void
): Promise<() => void> {
  return listen<AssistantStreamEvent>('assistant-stream-event', event => {
    handler(event.payload);
  });
}

export async function listAssistantPromptTemplates(): Promise<AssistantPromptTemplate[]> {
  return invoke<AssistantPromptTemplate[]>('list_assistant_prompt_templates');
}

export async function listAssistantSessions(): Promise<AssistantSession[]> {
  return invoke<AssistantSession[]>('list_assistant_sessions');
}

export async function listAssistantMessages(sessionId: string): Promise<AssistantMessage[]> {
  const id = sessionId.trim();
  if (!id) {
    throw new Error('会话 ID 不能为空');
  }
  return invoke<AssistantMessage[]>('list_assistant_messages', { sessionId: id });
}

export async function deleteAssistantSession(sessionId: string): Promise<void> {
  const id = sessionId.trim();
  if (!id) {
    throw new Error('会话 ID 不能为空');
  }
  await invoke('delete_assistant_session', { sessionId: id });
}

export async function deleteAssistantMessagesAfter(
  messageId: string,
  includeSelf: boolean
): Promise<void> {
  const id = messageId.trim();
  if (!id) {
    throw new Error('消息 ID 不能为空');
  }
  await invoke('delete_assistant_messages_after', {
    messageId: id,
    includeSelf,
  });
}

export async function updateAssistantUserMessageAndTruncate(
  messageId: string,
  content: string
): Promise<AssistantMessage> {
  const id = messageId.trim();
  const nextContent = content.trim();
  if (!id) {
    throw new Error('消息 ID 不能为空');
  }
  if (!nextContent) {
    throw new Error('请输入问题');
  }
  return invoke<AssistantMessage>('update_assistant_user_message_and_truncate', {
    messageId: id,
    content: nextContent,
  });
}

export async function saveAssistantAnswerAsNote(
  params: {
    workspaceId?: string;
    question: string;
    answer: string;
    scope: AssistantScope;
    sources: AssistantSource[];
    createdAt?: string;
  }
): Promise<NoteDoc> {
  assertAssistantScope(params.scope);
  const workspace = assertWorkspaceExists(params.workspaceId || getActiveLocalWorkspaceId());
  if (!params.question.trim() || !params.answer.trim()) {
    throw new Error('问题和回答不能为空');
  }

  return invoke<NoteDoc>('save_assistant_answer_note', {
    workspaceFolder: workspace.folderName,
    question: params.question,
    answer: params.answer,
    scope: params.scope,
    sources: params.sources,
    createdAt: params.createdAt || new Date().toISOString(),
  });
}

export async function saveAssistantSessionAsNote(
  sessionId: string,
  workspaceId = getActiveLocalWorkspaceId()
): Promise<NoteDoc> {
  const workspace = assertWorkspaceExists(workspaceId);
  const id = sessionId.trim();
  if (!id) {
    throw new Error('会话 ID 不能为空');
  }
  return invoke<NoteDoc>('save_assistant_session_note', {
    sessionId: id,
    workspaceFolder: workspace.folderName,
  });
}

export async function openExternalUrl(url: string): Promise<void> {
  const value = url.trim();
  if (!value) {
    throw new Error('网页链接为空');
  }
  await invoke('open_external_url', { url: value });
}
