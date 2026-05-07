/**
 * App.tsx - Local-only three-pane workspace layout
 */

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import './styles/tailwind-compat.css';

import { WorkspaceProvider, useWorkspace } from './contexts/WorkspaceContext';
import { useAppearance } from './hooks/useAppearance';
import { useLocalWorkspace } from './hooks/useLocalWorkspace';
import { useVoiceInputService } from './hooks/useServices';
import { AssistantSource } from './types';
import type { VoiceInputWarmupStatusEvent } from './appTypes';
import { openExternalUrl } from './lib/workspace';

import MeetingSidebar from './components/MeetingSidebar';
import RightSidebar from './components/RightSidebar';
import SettingsModal from './components/SettingsModal';
import EditorCenter from './components/EditorCenter';
import VoiceInputOverlay from './components/VoiceInputOverlay';

const RIGHT_SIDEBAR_WIDTH_KEY = 'layout:right-sidebar-width';
const DEFAULT_RIGHT_SIDEBAR_WIDTH = 360;
const MIN_RIGHT_SIDEBAR_WIDTH = 280;
const MAX_RIGHT_SIDEBAR_WIDTH = 720;
const VOICE_INPUT_OVERLAY_WINDOW_LABEL = 'voice-input-overlay';

function clampRightSidebarWidth(value: number) {
  return Math.min(MAX_RIGHT_SIDEBAR_WIDTH, Math.max(MIN_RIGHT_SIDEBAR_WIDTH, Math.round(value)));
}

function loadRightSidebarWidth() {
  const saved = Number(localStorage.getItem(RIGHT_SIDEBAR_WIDTH_KEY));
  return Number.isFinite(saved)
    ? clampRightSidebarWidth(saved)
    : DEFAULT_RIGHT_SIDEBAR_WIDTH;
}

function currentWindowLabel() {
  try {
    return getCurrentWindow().label;
  } catch {
    return 'main';
  }
}

function AppContent() {
  const {
    localWorkspaces,
    localWorkspace,
    switchToLocal,
    createLocalSpace,
    renameLocalSpace,
    removeLocalSpace,
    refreshLocalWorkspaceDirs,
  } = useWorkspace();
  const appearance = useAppearance();
  const local = useLocalWorkspace();
  const voiceInputService = useVoiceInputService();

  const [leftOpen, setLeftOpen] = useState<boolean>(true);
  const [rightOpen, setRightOpen] = useState<boolean>(true);
  const [rightSidebarWidth, setRightSidebarWidth] = useState<number>(loadRightSidebarWidth);
  const [rightTab, setRightTab] = useState<'recordings' | 'notes' | 'jobs' | 'assistant'>('recordings');
  const [showSettings, setShowSettings] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [webSearchNotice, setWebSearchNotice] = useState<boolean>(false);
  const [warmupNotice, setWarmupNotice] = useState<VoiceInputWarmupStatusEvent | null>(null);
  const handleClearError = useCallback(() => setError(null), []);
  const handleRightSidebarResize = useCallback((width: number) => {
    const nextWidth = clampRightSidebarWidth(width);
    setRightSidebarWidth(nextWidth);
    localStorage.setItem(RIGHT_SIDEBAR_WIDTH_KEY, String(nextWidth));
  }, []);
  const handleWebSearchEnabled = useCallback(() => {
    setWebSearchNotice(true);
  }, []);

  useEffect(() => {
    if (!webSearchNotice) return;
    const timer = window.setTimeout(() => setWebSearchNotice(false), 10_000);
    return () => window.clearTimeout(timer);
  }, [webSearchNotice]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void voiceInputService.onWarmupStatus(event => {
      setWarmupNotice(event);
    }).then(cleanup => {
      if (disposed) {
        cleanup();
        return;
      }
      unlisten = cleanup;
    }).catch(error => {
      console.warn('[VoiceInputWarmup] Failed to listen for warmup status', error);
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [voiceInputService]);

  useEffect(() => {
    if (!warmupNotice || warmupNotice.phase === 'warming') return;
    const timeoutMs = warmupNotice.phase === 'ready' ? 3_000 : 6_000;
    const timer = window.setTimeout(() => {
      setWarmupNotice(current => current === warmupNotice ? null : current);
    }, timeoutMs);
    return () => window.clearTimeout(timer);
  }, [warmupNotice]);

  const handleBeforeWorkspaceSummary = useCallback(async () => {
    await local.forceSaveActiveNoteNow();
    return local.refreshNotes(local.workspaceId);
  }, [local.forceSaveActiveNoteNow, local.refreshNotes, local.workspaceId]);

  const handleAssistantSourceOpen = useCallback(async (source: AssistantSource) => {
    if (source.type === 'web') {
      const url = source.url?.trim() || source.id?.trim();
      if (!url) {
        setError('网页引用缺少链接');
        return;
      }
      try {
        await openExternalUrl(url);
      } catch (error) {
        setError(error instanceof Error ? error.message : '打开网页失败');
      }
      return;
    }

    const noteId = source.note_id?.trim();
    if (!noteId) {
      setError('引用来源缺少笔记 ID');
      return;
    }
    const sourceWorkspaceFolder = source.workspace_folder?.trim();
    const targetWorkspace = sourceWorkspaceFolder
      ? localWorkspaces.find(workspace => workspace.folderName === sourceWorkspaceFolder)
      : localWorkspace;
    if (!targetWorkspace) {
      setError('引用笔记所在空间不存在');
      return;
    }

    const isActiveWorkspace = targetWorkspace.id === local.workspaceId;
    if (!isActiveWorkspace) {
      await local.switchWorkspace(targetWorkspace.id, { preferredNoteId: noteId });
      switchToLocal(targetWorkspace.id);
    }
    const notes = await local.refreshNotes(targetWorkspace.id, noteId);
    if (!notes.some(note => note.id === noteId)) {
      setError('引用笔记不存在或已被移动');
      return;
    }
    if (isActiveWorkspace) {
      await local.handleSelectNote(noteId);
    }
    setRightOpen(true);
    setRightTab('notes');
  }, [local, localWorkspace, localWorkspaces, switchToLocal]);

  const handleSelectLocalWorkspace = async (workspaceId: string) => {
    if (workspaceId === local.workspaceId) {
      switchToLocal(workspaceId);
      await Promise.all([
        local.refreshNotes(workspaceId),
        local.refreshRecordings(workspaceId),
      ]);
      return;
    }

    await local.switchWorkspace(workspaceId);
    switchToLocal(workspaceId);
  };

  const handleCreateLocalWorkspace = async (title: string) => {
    await local.forceSaveActiveNoteNow();
    const workspace = createLocalSpace(title);
    await local.switchWorkspace(workspace.id, { skipSave: true });
  };

  const handleRenameLocalWorkspace = (workspaceId: string, title: string) => {
    renameLocalSpace(workspaceId, title);
  };

  const handleDeleteLocalWorkspace = async (workspaceId: string) => {
    await local.forceSaveActiveNoteNow();
    const targetWorkspace = localWorkspaces.find(workspace => workspace.id === workspaceId);
    const nextWorkspace = localWorkspaces.find(workspace => workspace.id !== workspaceId);
    const nextWorkspaceId = nextWorkspace?.id || localWorkspace.id;
    try {
      if (targetWorkspace) {
        await invoke('delete_workspace_dir', { workspaceFolder: targetWorkspace.folderName });
      }
      removeLocalSpace(workspaceId);
      if (workspaceId === local.workspaceId) {
        await local.switchWorkspace(nextWorkspaceId, { skipSave: true });
        switchToLocal(nextWorkspaceId);
      }
      await refreshLocalWorkspaceDirs(nextWorkspaceId);
      if (workspaceId === local.workspaceId) {
        await local.refreshNotes(nextWorkspaceId);
      }
      await local.refreshRecordings(nextWorkspaceId);
    } catch (err) {
      setError(typeof err === 'string' ? err : '删除本地空间失败');
    }
  };

  return (
    <div className="app-container">
      {warmupNotice && (
        <div className={`voice-input-warmup-banner ${warmupBannerClass(warmupNotice.phase)}`} aria-live="polite">
          <div className="voice-input-warmup-content">
            <span className="voice-input-warmup-icon">{warmupBannerIcon(warmupNotice.phase)}</span>
            <div className="voice-input-warmup-text">
              <div className="voice-input-warmup-title">{warmupBannerTitle(warmupNotice.phase)}</div>
              <div className="voice-input-warmup-message">{warmupNotice.message}</div>
            </div>
            <button className="voice-input-warmup-close" onClick={() => setWarmupNotice(null)}>
              ×
            </button>
          </div>
        </div>
      )}

      {webSearchNotice && (
        <div className="warning-banner">
          <div className="warning-content">
            <span className="warning-icon">⚠</span>
            <div className="warning-text">
              <div className="warning-title">联网搜索已开启</div>
              <div className="warning-message">请保持当前网络环境可以访问 Google，否则搜索可能超时或无结果。</div>
            </div>
            <button className="warning-close" onClick={() => setWebSearchNotice(false)}>
              ×
            </button>
          </div>
        </div>
      )}

      {error && (
        <div className="error-banner">
          <div className="error-content">
            <span className="error-icon">⚠</span>
            <div className="error-text">
              <div className="error-title">Error</div>
              <div className="error-message">{error}</div>
            </div>
            <button className="error-close" onClick={() => setError(null)}>
              ×
            </button>
          </div>
        </div>
      )}

      <div className="workspace">
        <MeetingSidebar
          localWorkspaces={localWorkspaces}
          localWorkspace={localWorkspace}
          onSelectLocalWorkspace={handleSelectLocalWorkspace}
          onCreateLocalWorkspace={handleCreateLocalWorkspace}
          onRenameLocalWorkspace={handleRenameLocalWorkspace}
          onDeleteLocalWorkspace={handleDeleteLocalWorkspace}
          leftOpen={leftOpen}
          setLeftOpen={setLeftOpen}
          onOpenSettings={() => setShowSettings(true)}
        />

        <EditorCenter
          localWorkspace={localWorkspace}
          note={local.activeNote}
          onNewNote={local.newNote}
          onDeleteNote={local.deleteActiveNote}
          onChangeNoteContent={local.changeNoteContent}
          onRecordingSaved={local.onRecordingSaved}
          onError={setError}
          onClearError={handleClearError}
          darkMode={appearance.darkMode}
        />

        <RightSidebar
          rightOpen={rightOpen}
          setRightOpen={setRightOpen}
          width={rightSidebarWidth}
          minWidth={MIN_RIGHT_SIDEBAR_WIDTH}
          maxWidth={MAX_RIGHT_SIDEBAR_WIDTH}
          onResize={handleRightSidebarResize}
          rightTab={rightTab}
          setRightTab={setRightTab}
          recordings={local.recordings}
          meetingRecordingIds={local.workspaceRecordingIds}
          workspaceId={localWorkspace.id}
          workspaceFolder={localWorkspace.folderName}
          workspaceTitle={localWorkspace.title}
          refreshRecordings={local.refreshRecordings}
          onRecordingsImported={local.handleRecordingsImported}
          onTranscriptCreated={local.createNoteWithContent}
          onWorkspaceSummaryCreated={local.createNoteWithContent}
          onBeforeWorkspaceSummary={handleBeforeWorkspaceSummary}
          onError={setError}
          onClearError={handleClearError}
          onRecordingRenamed={local.handleRecordingRenamed}
          onRecordingDeleted={local.handleRecordingDeleted}
          notes={local.notes}
          activeNoteId={local.activeNoteId}
          onSelectNote={local.handleSelectNote}
          onRenameNote={local.renameNoteById}
          onExportNote={local.exportNoteAsMarkdown}
          onDeleteNote={local.deleteNoteFromList}
          onNotesImported={local.handleNotesImported}
          onRefreshNotes={local.handleRefreshNotes}
          onAssistantSourceOpen={handleAssistantSourceOpen}
          onWebSearchEnabled={handleWebSearchEnabled}
          darkMode={appearance.darkMode}
        />
      </div>

      <SettingsModal
        open={showSettings}
        onClose={() => setShowSettings(false)}
        darkMode={appearance.darkMode}
        setDarkMode={appearance.setDarkMode}
        fontSize={appearance.fontSize}
        setFontSize={appearance.setFontSize}
        onError={setError}
        onClearError={() => setError(null)}
      />
    </div>
  );
}

function warmupBannerClass(phase: string) {
  if (phase === 'ready') return 'success-banner';
  if (phase === 'skipped') return 'warning-banner';
  return 'info-banner';
}

function warmupBannerTitle(phase: string) {
  if (phase === 'ready') return '语音输入已就绪';
  if (phase === 'skipped') return '语音输入预热未完成';
  return '正在准备语音输入';
}

function warmupBannerIcon(phase: string) {
  if (phase === 'ready') return 'OK';
  if (phase === 'skipped') return '!';
  return 'ASR';
}

function App() {
  const isOverlayWindow = currentWindowLabel() === VOICE_INPUT_OVERLAY_WINDOW_LABEL;

  useEffect(() => {
    document.documentElement.classList.toggle('voice-input-overlay-window', isOverlayWindow);
    document.body.classList.toggle('voice-input-overlay-window', isOverlayWindow);
    return () => {
      document.documentElement.classList.remove('voice-input-overlay-window');
      document.body.classList.remove('voice-input-overlay-window');
    };
  }, [isOverlayWindow]);

  if (isOverlayWindow) {
    return <VoiceInputOverlay standalone />;
  }

  return (
    <WorkspaceProvider>
      <AppContent />
    </WorkspaceProvider>
  );
}

export default App;
