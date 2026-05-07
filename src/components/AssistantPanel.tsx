import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Bot,
  ChevronDown,
  ChevronUp,
  Edit3,
  FileText,
  Globe,
  Loader2,
  Plus,
  Save,
  Send,
  Trash2,
  X,
} from 'lucide-react';
import {
  AssistantMessage,
  AssistantPromptTemplate,
  AssistantRun,
  AssistantScope,
  AssistantSession,
  AssistantSource,
} from '../types';
import {
  askLocalNotesAgentStream,
  deleteAssistantMessagesAfter,
  deleteAssistantSession,
  getAssistantWorkspaceActivity,
  listenAssistantStream,
  listAssistantMessages,
  listAssistantPromptTemplates,
  listAssistantSessions,
  saveAssistantAnswerAsNote,
  saveAssistantSessionAsNote,
  updateAssistantUserMessageAndTruncate,
} from '../lib/workspace';
import { confirmLocalAction } from '../lib/confirm';
import BlockNoteEditorWithSource from './BlockNoteEditorWithSource';
import './AssistantPanel.css';

interface AssistantPanelProps {
  workspaceId: string;
  workspaceFolder?: string;
  onNoteSaved?: () => void;
  onSourceOpen?: (source: AssistantSource) => void | Promise<void>;
  onWebSearchEnabled?: () => void;
  onError?: (message: string) => void;
  darkMode?: boolean;
}

type PendingAssistantMessage = {
  id: string;
  session_id: string;
  role: 'assistant';
  content: string;
  scope: AssistantScope;
  sources: AssistantSource[];
  created_at: string;
  status: 'pending' | 'error';
};

type LocalUserMessage = AssistantMessage & {
  role: 'user';
  status: 'local';
};

type UiMessage = AssistantMessage | PendingAssistantMessage | LocalUserMessage;

type AssistantPanelOptions = {
  sessionId: string | null;
  scope: AssistantScope;
  promptTemplateId: string;
  webEnabled: boolean;
  maxTurns: string;
  toolbarCollapsed: boolean;
};

const ASSISTANT_MARKDOWN_FONT_SCALE = 0.875;
const ASSISTANT_PANEL_OPTIONS_PREFIX = 'assistant-panel-options:';
const DEFAULT_ASSISTANT_MAX_TURNS = 16;

function isPendingMessage(message: UiMessage): message is PendingAssistantMessage {
  return message.role === 'assistant' && 'status' in message;
}

function isLocalUserMessage(message: UiMessage): message is LocalUserMessage {
  return message.role === 'user' && 'status' in message && message.status === 'local';
}

function assistantPanelOptionsKey(workspaceId: string) {
  return `${ASSISTANT_PANEL_OPTIONS_PREFIX}${workspaceId}`;
}

function hasAssistantPanelOptions(workspaceId: string) {
  try {
    return localStorage.getItem(assistantPanelOptionsKey(workspaceId)) !== null;
  } catch {
    return false;
  }
}

function safeAssistantScope(value: unknown): AssistantScope {
  return value === 'global' ? 'global' : 'current';
}

function safeAssistantMaxTurns(value: unknown): string {
  const raw = String(value ?? '').trim();
  if (/^[1-9]\d*$/.test(raw)) return raw;
  const parsed = Number(raw);
  if (Number.isFinite(parsed) && parsed >= 1) return String(Math.trunc(parsed));
  return String(DEFAULT_ASSISTANT_MAX_TURNS);
}

function defaultAssistantPromptTemplateId(promptTemplates: AssistantPromptTemplate[]) {
  return promptTemplates.find(template => template.id === 'builtin-local-notes-qa')?.id || promptTemplates[0]?.id || '';
}

function resolveAssistantPromptTemplateId(value: string, promptTemplates: AssistantPromptTemplate[]) {
  const trimmed = value.trim();
  if (trimmed && promptTemplates.some(template => template.id === value)) {
    return trimmed;
  }
  return defaultAssistantPromptTemplateId(promptTemplates);
}

function loadAssistantPanelOptions(workspaceId: string): AssistantPanelOptions {
  const fallback: AssistantPanelOptions = {
    sessionId: null,
    scope: 'current',
    promptTemplateId: '',
    webEnabled: false,
    maxTurns: String(DEFAULT_ASSISTANT_MAX_TURNS),
    toolbarCollapsed: false,
  };
  try {
    const raw = localStorage.getItem(assistantPanelOptionsKey(workspaceId));
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Partial<AssistantPanelOptions>;
    return {
      sessionId: typeof parsed.sessionId === 'string' && parsed.sessionId.trim() ? parsed.sessionId : null,
      scope: safeAssistantScope(parsed.scope),
      promptTemplateId: typeof parsed.promptTemplateId === 'string' ? parsed.promptTemplateId : '',
      webEnabled: parsed.webEnabled === true,
      maxTurns: safeAssistantMaxTurns(parsed.maxTurns),
      toolbarCollapsed: parsed.toolbarCollapsed === true,
    };
  } catch {
    return fallback;
  }
}

function saveAssistantPanelOptions(workspaceId: string, options: AssistantPanelOptions) {
  try {
    localStorage.setItem(assistantPanelOptionsKey(workspaceId), JSON.stringify(options));
  } catch {
    // localStorage can be unavailable in restricted webviews; the panel still works without persistence.
  }
}

function dedupeAssistantMessages(items: UiMessage[]): UiMessage[] {
  const seen = new Set<string>();
  const deduped: UiMessage[] = [];
  for (const item of items) {
    if (seen.has(item.id)) continue;
    seen.add(item.id);
    deduped.push(item);
  }
  return deduped;
}

function pendingUserIdForRequest(requestId: string) {
  return `${requestId}:user`;
}

function runToPendingMessages(run: AssistantRun): [LocalUserMessage, PendingAssistantMessage] {
  return [
    {
      id: pendingUserIdForRequest(run.request_id),
      session_id: run.session_id,
      role: 'user',
      content: run.question,
      scope: run.scope,
      workspace_folder: run.workspace_folder,
      provider: run.provider,
      model: run.model,
      sources: [],
      created_at: run.created_at,
      status: 'local',
    },
    {
      id: run.request_id,
      session_id: run.session_id,
      role: 'assistant',
      content: run.partial_answer,
      scope: run.scope,
      sources: [],
      created_at: run.created_at,
      status: 'pending',
    },
  ];
}

function mergeAssistantDoneMessages(
  current: UiMessage[],
  requestId: string,
  userMessage: AssistantMessage,
  assistantMessage: AssistantMessage,
  localUserId?: string
): UiMessage[] {
  const pendingUserId = pendingUserIdForRequest(requestId);
  return dedupeAssistantMessages([
    ...current.filter(item => (
      item.id !== requestId &&
      item.id !== pendingUserId &&
      item.id !== localUserId &&
      item.id !== userMessage.id &&
      item.id !== assistantMessage.id
    )),
    userMessage,
    assistantMessage,
  ]);
}

function AssistantBubbleContent({ message, darkMode }: { message: UiMessage; darkMode: boolean }) {
  const isPending = isPendingMessage(message) && message.status === 'pending';
  const content = message.content || (isPending ? '正在读取本地笔记...' : '');

  if (message.role !== 'assistant') {
    return <>{content}</>;
  }

  return (
    <>
      <BlockNoteEditorWithSource
        value={content}
        onChange={() => undefined}
        noteId={message.id}
        mode="wysiwyg"
        darkMode={darkMode}
        readOnly
        compact
        fontScale={ASSISTANT_MARKDOWN_FONT_SCALE}
      />
      {isPending && <span className="assistant-caret" aria-hidden="true" />}
    </>
  );
}

export default function AssistantPanel({
  workspaceId,
  workspaceFolder,
  onNoteSaved,
  onSourceOpen,
  onWebSearchEnabled,
  onError,
  darkMode = false,
}: AssistantPanelProps) {
  const [sessions, setSessions] = useState<AssistantSession[]>([]);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [promptTemplates, setPromptTemplates] = useState<AssistantPromptTemplate[]>([]);
  const [promptTemplateId, setPromptTemplateId] = useState<string>('');
  const [messages, setMessages] = useState<UiMessage[]>([]);
  const [scope, setScope] = useState<AssistantScope>('current');
  const [webEnabled, setWebEnabled] = useState(false);
  const [maxTurns, setMaxTurns] = useState(String(DEFAULT_ASSISTANT_MAX_TURNS));
  const [usedTurns, setUsedTurns] = useState(0);
  const [toolbarCollapsed, setToolbarCollapsed] = useState(false);
  const [optionsReady, setOptionsReady] = useState(false);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const [savingId, setSavingId] = useState<string | null>(null);
  const [busyMessageId, setBusyMessageId] = useState<string | null>(null);
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [editingContent, setEditingContent] = useState('');
  const [hint, setHint] = useState<string | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);
  const activeRequestIdRef = useRef<string | null>(null);
  const activeRequestWorkspaceIdRef = useRef<string | null>(null);
  const activeWorkspaceIdRef = useRef(workspaceId);
  const loadedOptionsWereSavedRef = useRef(false);
  activeWorkspaceIdRef.current = workspaceId;

  const activeSession = useMemo(
    () => sessions.find(session => session.id === sessionId) || null,
    [sessions, sessionId]
  );

  const activeSessionLabel = activeSession?.title || '新会话';
  const effectivePromptTemplateId = resolveAssistantPromptTemplateId(promptTemplateId, promptTemplates);

  const isActiveAssistantRequest = useCallback((requestId: string, requestWorkspaceId: string) => {
    return (
      activeWorkspaceIdRef.current === requestWorkspaceId &&
      activeRequestWorkspaceIdRef.current === requestWorkspaceId &&
      activeRequestIdRef.current === requestId
    );
  }, []);

  const isActiveWorkspace = useCallback((requestWorkspaceId: string) => {
    return activeWorkspaceIdRef.current === requestWorkspaceId;
  }, []);

  const handleError = useCallback((error: unknown, fallback: string) => {
    const message = error instanceof Error ? error.message : typeof error === 'string' ? error : fallback;
    setHint(message);
    onError?.(message);
  }, [onError]);

  useEffect(() => {
    activeWorkspaceIdRef.current = workspaceId;
    loadedOptionsWereSavedRef.current = hasAssistantPanelOptions(workspaceId);
    const options = loadAssistantPanelOptions(workspaceId);
    setOptionsReady(false);
    activeRequestIdRef.current = null;
    activeRequestWorkspaceIdRef.current = null;
    setSending(false);
    setSavingId(null);
    setBusyMessageId(null);
    setHint(null);
    setSessionId(options.sessionId);
    setScope(options.scope);
    setPromptTemplateId(options.promptTemplateId);
    setWebEnabled(options.webEnabled);
    setMaxTurns(options.maxTurns);
    setUsedTurns(0);
    setToolbarCollapsed(options.toolbarCollapsed);
    setEditingMessageId(null);
    setEditingContent('');
    setOptionsReady(true);
  }, [workspaceId]);

  useEffect(() => {
    if (!optionsReady) return;
    saveAssistantPanelOptions(workspaceId, {
      sessionId,
      scope,
      promptTemplateId,
      webEnabled,
      maxTurns: safeAssistantMaxTurns(maxTurns),
      toolbarCollapsed,
    });
  }, [maxTurns, optionsReady, promptTemplateId, scope, sessionId, toolbarCollapsed, webEnabled, workspaceId]);

  useEffect(() => {
    if (!optionsReady) return;
    let cancelled = false;
    void (async () => {
      const optionsWereSaved = loadedOptionsWereSavedRef.current;
      const activity = workspaceFolder ? await getAssistantWorkspaceActivity(workspaceId) : { active_run: null, latest_session_id: null };
      const activeRun = activity.active_run || null;
      const preferredSessionId = activeRun?.session_id || (!optionsWereSaved ? activity.latest_session_id || null : undefined);
      const items = await listAssistantSessions();
      if (cancelled || !isActiveWorkspace(workspaceId)) return;
      setSessions(items);
      setSessionId(current => {
        const candidate = preferredSessionId !== undefined ? preferredSessionId : current;
        return candidate && items.some(item => item.id === candidate) ? candidate : null;
      });

      if (activity.active_run) {
        const activeRun = activity.active_run;
        activeRequestIdRef.current = activity.active_run.request_id;
        activeRequestWorkspaceIdRef.current = workspaceId;
        setSending(true);
        setSessionId(activeRun.session_id);
        setScope(activeRun.scope);
        setWebEnabled(activeRun.web_enabled);
        setMaxTurns(safeAssistantMaxTurns(activeRun.max_turns));
        setUsedTurns(Math.max(0, Math.trunc(Number(activeRun.current_turn) || 0)));
        if (activeRun.prompt_template_id) {
          setPromptTemplateId(activeRun.prompt_template_id);
        }
        const history = await listAssistantMessages(activeRun.session_id);
        if (cancelled || !isActiveWorkspace(workspaceId) || !isActiveAssistantRequest(activeRun.request_id, workspaceId)) return;
        setMessages(dedupeAssistantMessages([...history, ...runToPendingMessages(activeRun)]));
        return;
      }

      activeRequestIdRef.current = null;
      activeRequestWorkspaceIdRef.current = null;
      setSending(false);
      if (!optionsWereSaved && activity.latest_session_id && items.some(item => item.id === activity.latest_session_id)) {
        setSessionId(activity.latest_session_id);
        const messages = await listAssistantMessages(activity.latest_session_id);
        if (cancelled || !isActiveWorkspace(workspaceId) || activeRequestIdRef.current) return;
        setMessages(messages);
      }
    })().catch(error => {
      if (!cancelled) handleError(error, '加载 AI 问答会话失败');
    });
    return () => {
      cancelled = true;
    };
  }, [handleError, isActiveWorkspace, optionsReady, workspaceFolder, workspaceId]);

  useEffect(() => {
    let cancelled = false;
    void listAssistantPromptTemplates()
      .then(items => {
        if (cancelled) return;
        setPromptTemplates(items);
        setPromptTemplateId(current => resolveAssistantPromptTemplateId(current, items));
      })
      .catch(error => {
        if (!cancelled) handleError(error, '加载 AI 问答模板失败');
      });
    return () => {
      cancelled = true;
    };
  }, [handleError]);

  useEffect(() => {
    if (activeRequestIdRef.current) return;
    let cancelled = false;
    const targetSessionId = sessionId;
    void (async () => {
      const items = targetSessionId ? await listAssistantMessages(targetSessionId) : [];
      if (cancelled || activeRequestIdRef.current || !isActiveWorkspace(workspaceId)) return;
      setMessages(items);
    })()
      .catch(error => {
        if (!cancelled) handleError(error, '加载 AI 问答消息失败');
      });
    return () => {
      cancelled = true;
    };
  }, [handleError, isActiveWorkspace, sessionId, workspaceId]);

  useEffect(() => {
    if (!listRef.current) return;
    listRef.current.scrollTop = listRef.current.scrollHeight;
  }, [messages.length, messages[messages.length - 1]?.content]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void listenAssistantStream(event => {
      const activeRequestWorkspaceId = activeRequestWorkspaceIdRef.current;
      if (!event.request_id || !activeRequestWorkspaceId || !isActiveAssistantRequest(event.request_id, activeRequestWorkspaceId)) return;
      if (event.event === 'started') {
        if (event.session) {
          const session = event.session;
          setSessionId(session.id);
          setSessions(prev => {
            const rest = prev.filter(item => item.id !== session.id);
            return [session, ...rest];
          });
        }
        if (event.run) {
          activeRequestIdRef.current = event.run.request_id;
          setScope(event.run.scope);
          setWebEnabled(event.run.web_enabled);
          setMaxTurns(safeAssistantMaxTurns(event.run.max_turns));
          setUsedTurns(Math.max(0, Math.trunc(Number(event.run.current_turn) || 0)));
          if (event.run.prompt_template_id) {
            setPromptTemplateId(event.run.prompt_template_id);
          }
        }
      } else if (event.event === 'delta' && event.text) {
        setMessages(prev => prev.map(item => (
          isPendingMessage(item) && item.id === event.request_id
            ? { ...item, content: `${item.content}${event.text}` }
            : item
        )));
      } else if (event.event === 'turn') {
        const nextUsedTurns = Number(event.current_turn);
        if (Number.isFinite(nextUsedTurns)) {
          setUsedTurns(Math.max(0, Math.trunc(nextUsedTurns)));
        }
        if (event.max_turns !== undefined && event.max_turns !== null) {
          setMaxTurns(safeAssistantMaxTurns(event.max_turns));
        }
      } else if (event.event === 'tool') {
        const toolName = event.name || 'notes';
        const label = toolName.includes('web_search')
          ? '正在搜索网页'
          : toolName.includes('read_web_page')
            ? '正在读取网页'
            : `正在读取本地笔记：${toolName}`;
        setMessages(prev => prev.map(item => (
          isPendingMessage(item) && item.id === event.request_id
            ? { ...item, content: item.content || label }
            : item
        )));
      } else if (event.event === 'error') {
        setMessages(prev => prev.map(item => (
          isPendingMessage(item) && item.id === event.request_id
            ? { ...item, content: event.error || 'AI问答请求失败', status: 'error' }
            : item
        )));
        if (activeRequestIdRef.current === event.request_id) {
          activeRequestIdRef.current = null;
          activeRequestWorkspaceIdRef.current = null;
        }
        setSending(false);
      } else if (event.event === 'done' && event.user_message && event.assistant_message) {
        if (event.session) {
          const session = event.session;
          setSessionId(session.id);
          setSessions(prev => {
            const rest = prev.filter(item => item.id !== session.id);
            return [session, ...rest];
          });
        }
        setMessages(prev => mergeAssistantDoneMessages(prev, event.request_id, event.user_message!, event.assistant_message!));
        if (activeRequestIdRef.current === event.request_id) {
          activeRequestIdRef.current = null;
          activeRequestWorkspaceIdRef.current = null;
        }
        setSending(false);
      }
    }).then(cleanup => {
      if (cancelled) {
        cleanup();
        return;
      }
      unlisten = cleanup;
    }).catch(error => {
      handleError(error, '监听 AI 问答流失败');
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleError]);

  const canSubmit = input.trim().length > 0 && !sending && (scope !== 'current' || !!workspaceFolder);

  const toggleWebEnabled = useCallback(() => {
    setWebEnabled(value => {
      if (!value) {
        onWebSearchEnabled?.();
      }
      return !value;
    });
  }, [onWebSearchEnabled]);

  const handleNewSession = useCallback(() => {
    setSessionId(null);
    setMessages([]);
    setHint(null);
    setUsedTurns(0);
    setEditingMessageId(null);
    setEditingContent('');
  }, []);

  const handleSubmit = useCallback(async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const question = input.trim();
    if (!question || sending) return;
    if (scope !== 'current' && scope !== 'global') {
      handleError(new Error('AI问答范围无效'), 'AI问答范围无效');
      return;
    }
    if (scope === 'current' && !workspaceFolder) {
      handleError(new Error('当前空间模式必须指定本地空间'), '当前空间模式必须指定本地空间');
      return;
    }

    setInput('');
    setHint(null);
    setUsedTurns(0);
    setSending(true);
    const requestId = crypto.randomUUID();
    const requestWorkspaceId = workspaceId;
    activeRequestIdRef.current = requestId;
    activeRequestWorkspaceIdRef.current = requestWorkspaceId;
    const localSessionId = sessionId || 'pending-session';
    const userDraft: LocalUserMessage = {
      id: pendingUserIdForRequest(requestId),
      session_id: localSessionId,
      role: 'user',
      content: question,
      scope,
      workspace_folder: workspaceFolder,
      sources: [],
      created_at: new Date().toISOString(),
      status: 'local',
    };
    const assistantDraft: PendingAssistantMessage = {
      id: requestId,
      session_id: localSessionId,
      role: 'assistant',
      content: '',
      scope,
      sources: [],
      created_at: new Date().toISOString(),
      status: 'pending',
    };
    setMessages(prev => [...prev, userDraft, assistantDraft]);

    try {
      const result = await askLocalNotesAgentStream({
        requestId,
        sessionId,
        question,
        scope,
        workspaceId,
        promptTemplateId: effectivePromptTemplateId || null,
        webEnabled,
        maxTurns: safeAssistantMaxTurns(maxTurns),
      });
      if (!isActiveAssistantRequest(requestId, requestWorkspaceId)) return;
      setSessionId(result.session.id);
      setSessions(prev => {
        const rest = prev.filter(item => item.id !== result.session.id);
        return [result.session, ...rest];
      });
      setMessages(prev => mergeAssistantDoneMessages(prev, requestId, result.user_message, result.assistant_message));
    } catch (error) {
      if (!isActiveAssistantRequest(requestId, requestWorkspaceId)) return;
      setMessages(prev => prev.map(item => (
        item.id === assistantDraft.id
          ? { ...item, content: '暂时无法获取回复，请检查 LLM 配置或稍后重试。', status: 'error' }
          : item
      )));
      handleError(error, 'AI问答请求失败');
    } finally {
      if (isActiveAssistantRequest(requestId, requestWorkspaceId)) {
        activeRequestIdRef.current = null;
        activeRequestWorkspaceIdRef.current = null;
        setSending(false);
      }
    }
  }, [effectivePromptTemplateId, handleError, input, isActiveAssistantRequest, maxTurns, scope, sending, sessionId, webEnabled, workspaceFolder, workspaceId]);

  const saveAnswer = useCallback(async (assistantMessage: UiMessage, index: number) => {
    const requestWorkspaceId = workspaceId;
    const previousUser = [...messages.slice(0, index)]
      .reverse()
      .find(message => message.role === 'user');
    if (!previousUser || assistantMessage.role !== 'assistant' || isPendingMessage(assistantMessage)) return;
    setSavingId(assistantMessage.id);
    try {
      await saveAssistantAnswerAsNote({
        workspaceId,
        question: previousUser.content,
        answer: assistantMessage.content,
        scope: assistantMessage.scope,
        sources: assistantMessage.sources,
        createdAt: assistantMessage.created_at,
      });
      if (!isActiveWorkspace(requestWorkspaceId)) return;
      setHint('回答已保存为笔记');
      onNoteSaved?.();
    } catch (error) {
      if (isActiveWorkspace(requestWorkspaceId)) handleError(error, '保存回答为笔记失败');
    } finally {
      if (isActiveWorkspace(requestWorkspaceId)) setSavingId(null);
    }
  }, [handleError, isActiveWorkspace, messages, onNoteSaved, workspaceId]);

  const saveSession = useCallback(async () => {
    if (!sessionId) return;
    const requestWorkspaceId = workspaceId;
    setSavingId('session');
    try {
      await saveAssistantSessionAsNote(sessionId, workspaceId);
      if (!isActiveWorkspace(requestWorkspaceId)) return;
      setHint('会话已保存为笔记');
      onNoteSaved?.();
    } catch (error) {
      if (isActiveWorkspace(requestWorkspaceId)) handleError(error, '保存会话为笔记失败');
    } finally {
      if (isActiveWorkspace(requestWorkspaceId)) setSavingId(null);
    }
  }, [handleError, isActiveWorkspace, onNoteSaved, sessionId, workspaceId]);

  const handleDeleteSession = useCallback(async () => {
    if (!sessionId || sending) return;
    const requestWorkspaceId = workspaceId;
    const accepted = await confirmLocalAction('删除当前 AI 问答会话后，历史消息也会一并删除。', {
      title: '删除 AI 问答会话',
      confirmLabel: '删除',
      destructive: true,
    });
    if (!accepted || !isActiveWorkspace(requestWorkspaceId)) return;
    setSavingId('delete-session');
    try {
      await deleteAssistantSession(sessionId);
      if (!isActiveWorkspace(requestWorkspaceId)) return;
      const remaining = sessions.filter(session => session.id !== sessionId);
      const nextSessionId = remaining[0]?.id || null;
      setSessions(remaining);
      setSessionId(nextSessionId);
      setMessages([]);
      setHint('会话已删除');
    } catch (error) {
      if (isActiveWorkspace(requestWorkspaceId)) handleError(error, '删除 AI 问答会话失败');
    } finally {
      if (isActiveWorkspace(requestWorkspaceId)) setSavingId(null);
    }
  }, [handleError, isActiveWorkspace, sending, sessionId, sessions, workspaceId]);

  const handleDeleteUserMessage = useCallback(async (message: UiMessage, index: number) => {
    if (sending) return;
    const requestWorkspaceId = workspaceId;
    const accepted = await confirmLocalAction('删除这条用户输入后，它之后的回答和追问也会一并删除。', {
      title: '删除历史对话',
      confirmLabel: '删除',
      destructive: true,
    });
    if (!accepted || !isActiveWorkspace(requestWorkspaceId)) return;
    if (isLocalUserMessage(message)) {
      setMessages(prev => prev.slice(0, index));
      setEditingMessageId(null);
      setEditingContent('');
      setHint('历史对话已删除');
      return;
    }
    setBusyMessageId(message.id);
    try {
      await deleteAssistantMessagesAfter(message.id, true);
      const messages = await listAssistantMessages(message.session_id);
      if (!isActiveWorkspace(requestWorkspaceId)) return;
      setMessages(messages);
      setHint('历史对话已删除');
    } catch (error) {
      if (isActiveWorkspace(requestWorkspaceId)) handleError(error, '删除历史对话失败');
    } finally {
      if (isActiveWorkspace(requestWorkspaceId)) setBusyMessageId(null);
    }
  }, [handleError, isActiveWorkspace, sending, workspaceId]);

  const startEditUserMessage = useCallback((message: UiMessage) => {
    setEditingMessageId(message.id);
    setEditingContent(message.content);
    setHint(null);
  }, []);

  const cancelEditUserMessage = useCallback(() => {
    setEditingMessageId(null);
    setEditingContent('');
  }, []);

  const submitEditedUserMessage = useCallback(async (message: UiMessage, index: number) => {
    const question = editingContent.trim();
    if (!question || sending) return;
    const requestWorkspaceId = workspaceId;
    const accepted = await confirmLocalAction('重新提问会丢弃这条输入之后的消息，并从修改后的内容继续生成。', {
      title: '重新生成后续对话',
      confirmLabel: '重新提问',
      destructive: true,
    });
    if (!accepted || !isActiveWorkspace(requestWorkspaceId)) return;

    setSending(true);
    setBusyMessageId(message.id);
    setHint(null);
    setUsedTurns(0);
    let assistantDraftId: string | null = null;
    try {
      const requestId = crypto.randomUUID();
      assistantDraftId = requestId;
      activeRequestIdRef.current = requestId;
      activeRequestWorkspaceIdRef.current = requestWorkspaceId;
      const updatedUser = isLocalUserMessage(message)
        ? {
            ...message,
            content: question,
          }
        : await updateAssistantUserMessageAndTruncate(message.id, question);
      const assistantDraft: PendingAssistantMessage = {
        id: requestId,
        session_id: updatedUser.session_id,
        role: 'assistant',
        content: '',
        scope,
        sources: [],
        created_at: new Date().toISOString(),
        status: 'pending',
      };
      setEditingMessageId(null);
      setEditingContent('');
      setMessages(prev => [...prev.slice(0, index), updatedUser, assistantDraft]);

      const result = await askLocalNotesAgentStream({
        requestId,
        sessionId: isLocalUserMessage(updatedUser)
          ? (updatedUser.session_id === 'pending-session' ? null : updatedUser.session_id)
          : updatedUser.session_id,
        question,
        scope,
        workspaceId,
        promptTemplateId: effectivePromptTemplateId || null,
        webEnabled,
        maxTurns: safeAssistantMaxTurns(maxTurns),
        reuseUserMessageId: isLocalUserMessage(updatedUser) ? undefined : updatedUser.id,
      });
      if (!isActiveAssistantRequest(requestId, requestWorkspaceId)) return;
      setSessionId(result.session.id);
      setSessions(prev => {
        const rest = prev.filter(item => item.id !== result.session.id);
        return [result.session, ...rest];
      });
      setMessages(prev => mergeAssistantDoneMessages(
        prev,
        requestId,
        result.user_message,
        result.assistant_message,
        updatedUser.id
      ));
    } catch (error) {
      if (!assistantDraftId || !isActiveAssistantRequest(assistantDraftId, requestWorkspaceId)) return;
      if (assistantDraftId) {
        setMessages(prev => prev.map(item => (
          item.id === assistantDraftId
            ? { ...item, content: '暂时无法获取回复，请检查 LLM 配置或稍后重试。', status: 'error' }
            : item
        )));
      }
      handleError(error, '重新生成 AI 问答失败');
    } finally {
      if (assistantDraftId && isActiveAssistantRequest(assistantDraftId, requestWorkspaceId)) {
        activeRequestIdRef.current = null;
        activeRequestWorkspaceIdRef.current = null;
        setBusyMessageId(null);
        setSending(false);
      }
    }
  }, [editingContent, effectivePromptTemplateId, handleError, isActiveAssistantRequest, isActiveWorkspace, maxTurns, scope, sending, webEnabled, workspaceId]);

  return (
    <div className="assistant-panel">
      <div className={`assistant-toolbar ${toolbarCollapsed ? 'collapsed' : ''}`}>
        {toolbarCollapsed ? (
          <div className="assistant-toolbar-row assistant-toolbar-compact-row">
            <button
              type="button"
              className="toolbar-icon-btn assistant-toolbar-btn"
              onClick={() => setToolbarCollapsed(false)}
              title="展开 AI 问答工具栏"
            >
              <ChevronDown size={14} />
            </button>
            <div className="assistant-toolbar-summary" title={activeSessionLabel}>
              {activeSessionLabel}
              <span>{scope === 'current' ? '当前空间' : '全部空间'}</span>
            </div>
            <label className={`assistant-switch compact ${sending ? 'disabled' : ''}`} title={webEnabled ? '关闭联网搜索' : '开启联网搜索'}>
              <input
                type="checkbox"
                role="switch"
                checked={webEnabled}
                onChange={toggleWebEnabled}
                disabled={sending}
              />
              <span className="assistant-switch-track" aria-hidden="true" />
              <span className="assistant-switch-label">联网</span>
            </label>
          </div>
        ) : (
          <>
            <div className="assistant-toolbar-row">
              <select
                className="assistant-session-select"
                value={sessionId || ''}
                onChange={event => {
                  const value = event.target.value;
                  setSessionId(value || null);
                  setUsedTurns(0);
                  setEditingMessageId(null);
                  setEditingContent('');
                }}
                disabled={sending}
                title="AI问答会话"
              >
                <option value="">新会话</option>
                {sessions.map(session => (
                  <option key={session.id} value={session.id}>{session.title}</option>
                ))}
              </select>
              <select
                className="assistant-template-select"
                value={promptTemplateId}
                onChange={event => setPromptTemplateId(event.target.value)}
                disabled={sending || promptTemplates.length === 0}
                title="AI问答 Prompt 模板"
              >
                {promptTemplates.length === 0 ? (
                  <option value="">无可用模板</option>
                ) : (
                  promptTemplates.map(template => (
                    <option key={template.id} value={template.id}>{template.name}</option>
                  ))
                )}
              </select>
              <button
                type="button"
                className="toolbar-icon-btn assistant-toolbar-btn"
                onClick={handleNewSession}
                disabled={sending}
                title="新建会话"
              >
                <Plus size={14} />
              </button>
              <button
                type="button"
                className="toolbar-icon-btn assistant-toolbar-btn danger"
                onClick={() => void handleDeleteSession()}
                disabled={!activeSession || sending || savingId !== null}
                title="删除当前会话"
              >
                {savingId === 'delete-session' ? <Loader2 size={14} className="spinning" /> : <Trash2 size={14} />}
              </button>
            </div>
            <div className="assistant-toolbar-row">
              <div className="assistant-scope-toggle" role="group" aria-label="AI问答范围">
                <button
                  type="button"
                  className={scope === 'current' ? 'active' : ''}
                  onClick={() => setScope('current')}
                  disabled={sending}
                >
                  当前空间
                </button>
                <button
                  type="button"
                  className={scope === 'global' ? 'active' : ''}
                  onClick={() => setScope('global')}
                  disabled={sending}
                >
                  全部空间
                </button>
              </div>
              <label className={`assistant-switch ${sending ? 'disabled' : ''}`} title={webEnabled ? '关闭联网搜索' : '开启联网搜索'}>
                <input
                  type="checkbox"
                  role="switch"
                  checked={webEnabled}
                  onChange={toggleWebEnabled}
                  disabled={sending}
                />
                <span className="assistant-switch-track" aria-hidden="true" />
                <span className="assistant-switch-label">联网</span>
              </label>
              <label className="assistant-max-turns" title="AI 工具调用轮次">
                <span>轮次</span>
                <input
                  type="number"
                  min={1}
                  step={1}
                  value={maxTurns}
                  onChange={event => setMaxTurns(event.target.value)}
                  onBlur={() => setMaxTurns(String(safeAssistantMaxTurns(maxTurns)))}
                  disabled={sending}
                />
              </label>
              <span className="assistant-turn-usage" title="本次 AI 工具调用轮次">
                已用 {usedTurns} / {safeAssistantMaxTurns(maxTurns)}
              </span>
            </div>
            <div className="assistant-toolbar-row assistant-toolbar-meta">
              <span>已按当前空间记住会话、范围、模板、联网和轮次</span>
              <button
                type="button"
                className="toolbar-icon-btn assistant-toolbar-btn"
                onClick={() => setToolbarCollapsed(true)}
                title="折叠 AI 问答工具栏"
              >
                <ChevronUp size={14} />
              </button>
            </div>
          </>
        )}
      </div>

      <div className="assistant-messages" ref={listRef}>
        {messages.length === 0 && (
          <div className="assistant-empty">
            <Bot size={20} />
            <span>选择范围后提问本地笔记内容</span>
          </div>
        )}
        {messages.map((message, index) => {
          const isEditing = message.role === 'user' && !isPendingMessage(message) && editingMessageId === message.id;
          return (
            <div key={message.id} className={`assistant-message ${message.role} ${isPendingMessage(message) ? message.status : ''}`}>
              {message.role === 'assistant' && (
                <div className="assistant-avatar">
                  <Bot size={16} />
                </div>
              )}
              <div className="assistant-bubble-wrap">
                {isEditing ? (
                  <div className="assistant-edit-card">
                    <textarea
                      className="assistant-edit-textarea"
                      value={editingContent}
                      onChange={event => setEditingContent(event.target.value)}
                      disabled={sending}
                      autoFocus
                    />
                    <div className="assistant-edit-actions">
                      <button
                        type="button"
                        className="modal-secondary assistant-edit-button"
                        onClick={cancelEditUserMessage}
                        disabled={sending}
                      >
                        取消
                      </button>
                      <button
                        type="button"
                        className="modal-primary assistant-edit-button"
                        onClick={() => void submitEditedUserMessage(message, index)}
                        disabled={sending || editingContent.trim().length === 0}
                      >
                        重新提问
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="assistant-bubble">
                    <AssistantBubbleContent message={message} darkMode={darkMode} />
                  </div>
                )}
                {message.role === 'user' && !isPendingMessage(message) && !isEditing && (
                  <div className="assistant-message-actions user-actions">
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={() => startEditUserMessage(message)}
                      disabled={sending || busyMessageId !== null}
                      title="修改这条提问"
                    >
                      <Edit3 size={13} />
                    </button>
                    <button
                      type="button"
                      className="mini-btn danger"
                      onClick={() => void handleDeleteUserMessage(message, index)}
                      disabled={sending || busyMessageId !== null}
                      title="删除这条及之后的对话"
                    >
                      {busyMessageId === message.id ? <Loader2 size={13} className="spinning" /> : <Trash2 size={13} />}
                    </button>
                  </div>
                )}
                {message.role === 'assistant' && !isPendingMessage(message) && (
                  <div className="assistant-message-actions">
                    <button
                      type="button"
                      className="mini-btn"
                      onClick={() => void saveAnswer(message, index)}
                      disabled={savingId !== null}
                      title="保存回答为笔记"
                    >
                      {savingId === message.id ? <Loader2 size={14} className="spinning" /> : <Save size={14} />}
                    </button>
                  </div>
                )}
                {message.role === 'assistant' && message.sources.length > 0 && (
                  <div className="assistant-sources" aria-label="引用来源">
                    {message.sources.map(source => (
                      <button
                        key={`${source.workspace_folder || ''}:${source.note_id}:${source.id}`}
                        type="button"
                        onClick={() => void onSourceOpen?.(source)}
                        title={source.type === 'web' ? `打开网页：${source.url || source.id}` : `打开引用笔记：${source.title}`}
                      >
                        {source.type === 'web' ? <Globe size={13} /> : <FileText size={13} />}
                        <span>{source.title}</span>
                      </button>
                    ))}
                  </div>
                )}
              </div>
              {message.role === 'user' && isEditing && (
                <button
                  type="button"
                  className="mini-btn assistant-edit-close"
                  onClick={cancelEditUserMessage}
                  title="取消编辑"
                  disabled={sending}
                >
                  <X size={13} />
                </button>
              )}
            </div>
          );
        })}
      </div>

      {hint && <div className="assistant-hint">{hint}</div>}

      <div className="assistant-footer-actions">
        <button
          type="button"
          className="modal-secondary assistant-save-session"
          onClick={saveSession}
          disabled={!activeSession || messages.length === 0 || savingId !== null || sending}
          title="保存整个会话为笔记"
        >
          {savingId === 'session' ? <Loader2 size={14} className="spinning" /> : <Save size={14} />}
          保存会话
        </button>
      </div>

      <form className="assistant-input-row" onSubmit={handleSubmit}>
        <input
          className="assistant-input"
          value={input}
          placeholder={scope === 'current' ? '询问当前本地笔记' : '询问全部本地空间笔记'}
          onChange={event => setInput(event.target.value)}
          disabled={sending}
        />
        <button className="toolbar-icon-btn assistant-send" type="submit" disabled={!canSubmit}>
          {sending ? <Loader2 size={16} className="spinning" /> : <Send size={16} />}
        </button>
      </form>
    </div>
  );
}
