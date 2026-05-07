import { useCallback, useEffect, useRef, useState } from 'react';
import { List, FileText, ChevronLeft, ChevronRight, Bot, ClipboardList } from 'lucide-react';
import { AssistantSource, NoteDoc, Recording, RightPaneTab } from '../types';
import RecordingList from './RecordingList';
import NotesList from './NotesList';
import AssistantPanel from './AssistantPanel';
import LocalJobPanel from './LocalJobPanel';
import { useLocalQueueSync } from '../hooks/useLocalQueueSync';

interface Props {
  rightOpen: boolean;
  setRightOpen: (open: boolean) => void;
  width: number;
  minWidth: number;
  maxWidth: number;
  onResize: (width: number) => void;
  rightTab: RightPaneTab;
  setRightTab: (tab: RightPaneTab) => void;

  recordings: Recording[];
  meetingRecordingIds: string[];
  workspaceId?: string;
  workspaceFolder?: string;
  workspaceTitle?: string;
  refreshRecordings: () => Promise<Recording[]>;
  onRecordingsImported?: (recordingIds: string[]) => void;
  onTranscriptCreated?: (title: string, content: string) => unknown | Promise<unknown>;
  onWorkspaceSummaryCreated?: (title: string, content: string) => unknown | Promise<unknown>;
  onBeforeWorkspaceSummary?: () => Promise<NoteDoc[] | void> | NoteDoc[] | void;
  onRecordingDeleted?: (recordingId: string) => void;
  onError: (msg: string) => void;
  onClearError: () => void;
  onRecordingRenamed: (oldId: string, newRecording: Recording) => void;

  notes: NoteDoc[];
  activeNoteId: string | null;
  onSelectNote: (id: string) => void;
  onRenameNote: (id: string, title: string) => void;
  onExportNote: (note: NoteDoc) => void;
  onDeleteNote: (note: NoteDoc) => void;
  onNotesImported?: (notes: NoteDoc[]) => void | Promise<void>;
  onRefreshNotes?: () => void;
  onAssistantSourceOpen?: (source: AssistantSource) => void | Promise<void>;
  onWebSearchEnabled?: () => void;
  darkMode?: boolean;
}

export default function RightSidebar(props: Props) {
  const { rightOpen, setRightOpen, rightTab, setRightTab } = props;
  const [resizing, setResizing] = useState(false);
  const resizeStateRef = useRef<{ startX: number; startWidth: number } | null>(null);

  const handleJobsChanged = useCallback(async () => {
    await props.refreshRecordings();
    props.onRefreshNotes?.();
  }, [props.refreshRecordings, props.onRefreshNotes]);

  const beginResize = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (!rightOpen) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    resizeStateRef.current = {
      startX: event.clientX,
      startWidth: props.width,
    };
    setResizing(true);
  }, [props.width, rightOpen]);

  const updateResize = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    const state = resizeStateRef.current;
    if (!state) return;
    const nextWidth = state.startWidth + (state.startX - event.clientX);
    props.onResize(nextWidth);
  }, [props.onResize]);

  const endResize = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (!resizeStateRef.current) return;
    resizeStateRef.current = null;
    setResizing(false);
    try {
      event.currentTarget.releasePointerCapture(event.pointerId);
    } catch {
      // Pointer capture may already be released by the browser.
    }
  }, []);

  useEffect(() => {
    document.body.classList.toggle('resizing-right-sidebar', resizing);
    return () => document.body.classList.remove('resizing-right-sidebar');
  }, [resizing]);

  useLocalQueueSync({
    workspaceFolder: props.workspaceFolder,
    onTranscriptCreated: props.onTranscriptCreated,
    onWorkspaceSummaryCreated: props.onWorkspaceSummaryCreated,
    onError: props.onError,
    onJobsChanged: handleJobsChanged,
  });
  const tabBtn = (id: RightPaneTab, label: string, Icon: any) => (
    <button className={`tab-btn ${rightTab === id ? 'active' : ''}`} onClick={() => setRightTab(id)}>
      <Icon size={16} /> {label}
    </button>
  );

  return (
    <aside
      className={`right-sidebar ${rightOpen ? 'open' : 'collapsed'} ${resizing ? 'resizing' : ''}`}
      style={rightOpen ? { width: props.width } : undefined}
    >
      {rightOpen && (
        <div
          className="right-resize-handle"
          role="separator"
          aria-orientation="vertical"
          aria-label="调整右侧栏宽度"
          aria-valuenow={props.width}
          aria-valuemin={props.minWidth}
          aria-valuemax={props.maxWidth}
          onPointerDown={beginResize}
          onPointerMove={updateResize}
          onPointerUp={endResize}
          onPointerCancel={endResize}
          onDoubleClick={() => props.onResize(360)}
        />
      )}
      <div className="right-header">
        <div className="tabs">
          {tabBtn('recordings', '录音', List)}
          {tabBtn('notes', '笔记', FileText)}
          {tabBtn('jobs', '任务', ClipboardList)}
          {tabBtn('assistant', 'AI问答', Bot)}
        </div>
        <button className="icon-btn collapse-toggle" onClick={() => setRightOpen(!rightOpen)} title={rightOpen ? '收起' : '展开'}>
          {rightOpen ? <ChevronRight size={18} /> : <ChevronLeft size={18} />}
        </button>
      </div>

      {rightOpen && (
        rightTab === 'recordings' ? (
          <RecordingList
            key={`recordings:${props.workspaceId || props.workspaceFolder || 'local'}`}
            recordings={props.recordings}
            meetingRecordingIds={props.meetingRecordingIds}
            workspaceFolder={props.workspaceFolder}
            workspaceTitle={props.workspaceTitle}
            refreshRecordings={props.refreshRecordings}
            onError={props.onError}
            onClearError={props.onClearError}
            onRecordingRenamed={props.onRecordingRenamed}
            onRecordingDeleted={props.onRecordingDeleted}
            onRecordingsImported={props.onRecordingsImported}
            onTranscriptCreated={props.onTranscriptCreated}
            onWorkspaceSummaryCreated={props.onWorkspaceSummaryCreated}
          />
        ) : rightTab === 'notes' ? (
          <NotesList
            key={`notes:${props.workspaceId || props.workspaceFolder || 'local'}`}
            notes={props.notes}
            activeNoteId={props.activeNoteId}
            onSelect={props.onSelectNote}
            onRename={props.onRenameNote}
            onExport={props.onExportNote}
            onDelete={props.onDeleteNote}
            onNotesImported={props.onNotesImported}
            onError={props.onError}
            onRefresh={props.onRefreshNotes}
            workspaceFolder={props.workspaceFolder}
            workspaceTitle={props.workspaceTitle}
            onBeforeWorkspaceSummary={props.onBeforeWorkspaceSummary}
          />
        ) : rightTab === 'jobs' ? (
          <LocalJobPanel
            workspaceFolder={props.workspaceFolder}
            onError={props.onError}
            onJobsChanged={handleJobsChanged}
          />
        ) : (
          <AssistantPanel
            workspaceId={props.workspaceId || 'local-workspace'}
            workspaceFolder={props.workspaceFolder}
            onNoteSaved={props.onRefreshNotes}
            onSourceOpen={props.onAssistantSourceOpen}
            onWebSearchEnabled={props.onWebSearchEnabled}
            onError={props.onError}
            darkMode={props.darkMode}
          />
        )
      )}
    </aside>
  );
}
